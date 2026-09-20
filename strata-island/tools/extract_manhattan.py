#!/usr/bin/env python3
"""Generate fixtures/manhattan-drive.json from OSM (Overpass).

Recipe is frozen in docs/implementation-plan.md. Do not retune filters
to hunt a node-count band. Record whatever this recipe produces.
"""

from __future__ import annotations

import json
import math
import sys
import urllib.request
from collections import defaultdict
from pathlib import Path

SOUTH, NORTH, WEST, EAST = 40.70, 40.88, -74.03, -73.91
ORIGIN_LAT, ORIGIN_LON = 40.7003, -74.0170
METERS_PER_DEG_LAT = 110_540.0
METERS_PER_DEG_LON = 111_320.0 * math.cos(math.radians(ORIGIN_LAT))
# Keep-near-pin during collapse (plan: 40 m). Post-collapse gazetteer/corridor
# snap uses 200 m so the nearest *kept* intersection still binds after chains
# collapse away the vertex that sat on the WGS84 pin.
KEEP_PIN_M2 = 40 * 40
SNAP_POI_M2 = 200 * 200

ALLOW = {
    "motorway",
    "motorway_link",
    "trunk",
    "trunk_link",
    "primary",
    "primary_link",
    "secondary",
    "secondary_link",
    "tertiary",
    "tertiary_link",
    "unclassified",
    "residential",
    "living_street",
}
DROP = {
    "service",
    "footway",
    "path",
    "pedestrian",
    "cycleway",
    "steps",
    "bridleway",
    "track",
    "construction",
    "proposed",
    "corridor",
    "platform",
    "raceway",
    "busway",
    "bus_guideway",
    "emergency_bay",
    "abandoned",
    "disused",
    "rest_area",
    "services",
    "escape",
}

PINS = {
    "west_42": (40.7558, -73.9903),
    "east_42": (40.7540, -73.9816),
    "park_42": (40.7527, -73.9772),
    "battery": (40.7033, -74.0170),
    "empire": (40.7484, -73.9857),
    "bellevue": (40.7394, -73.9756),
    "lincoln": (40.7614, -73.9980),
}

OVERPASS = "https://overpass-api.de/api/interpreter"
QUERY = f"""
[out:json][timeout:240];
(
  way["highway"~"^(motorway|motorway_link|trunk|trunk_link|primary|primary_link|secondary|secondary_link|tertiary|tertiary_link|unclassified|residential|living_street)$"]({SOUTH},{WEST},{NORTH},{EAST});
);
(._;>;);
out;
"""


def project(lat: float, lon: float) -> tuple[int, int]:
    x = round((lon - ORIGIN_LON) * METERS_PER_DEG_LON)
    y = round((lat - ORIGIN_LAT) * METERS_PER_DEG_LAT)
    return int(x), int(y)


def in_bbox(lat: float, lon: float) -> bool:
    return SOUTH <= lat <= NORTH and WEST <= lon <= EAST


def dist2(a: tuple[int, int], b: tuple[int, int]) -> int:
    dx = a[0] - b[0]
    dy = a[1] - b[1]
    return dx * dx + dy * dy


def fetch() -> dict:
    import urllib.parse

    body = urllib.parse.urlencode({"data": QUERY}).encode()
    last_err: Exception | None = None
    for url in (
        OVERPASS,
        "https://overpass.kumi.systems/api/interpreter",
        "https://overpass.private.coffee/api/interpreter",
    ):
        req = urllib.request.Request(
            url,
            data=body,
            headers={
                "Content-Type": "application/x-www-form-urlencoded",
                "User-Agent": "strata-island/0.1 (manhattan extract; local demo)",
            },
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=300) as resp:
                return json.load(resp)
        except Exception as err:  # noqa: BLE001 — try the next mirror
            last_err = err
            print(f"{url}: {err}", file=sys.stderr)
    raise last_err or RuntimeError("overpass failed")


def oneway_dirs(tags: dict) -> list[int]:
    hw = tags.get("highway", "")
    jn = tags.get("junction", "")
    ow = str(tags.get("oneway", "")).lower()
    if ow in {"yes", "true", "1"}:
        return [1]
    if ow in {"-1", "reverse"}:
        return [-1]
    if jn in {"roundabout", "circular"}:
        return [1]
    if ow in {"no", "false", "0", ""}:
        return [1, -1]
    return [1, -1]


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    out_city = root / "fixtures" / "manhattan-drive.json"
    out_close = root / "fixtures" / "closure-42nd.json"
    out_poi = root / "fixtures" / "gazetteer.json"

    print("fetching Overpass…", file=sys.stderr)
    payload = fetch()
    elements = payload.get("elements", [])
    nodes_ll: dict[int, tuple[float, float]] = {}
    ways = []
    for el in elements:
        if el.get("type") == "node":
            nodes_ll[el["id"]] = (float(el["lat"]), float(el["lon"]))
        elif el.get("type") == "way":
            ways.append(el)

    # Drop ways with excluded highway / area / any node outside bbox.
    kept_ways = []
    for way in ways:
        tags = way.get("tags") or {}
        hw = tags.get("highway", "")
        if hw not in ALLOW or hw in DROP:
            continue
        if tags.get("area") == "yes":
            continue
        refs = way.get("nodes") or []
        if len(refs) < 2:
            continue
        ok = True
        for nid in refs:
            ll = nodes_ll.get(nid)
            if ll is None or not in_bbox(*ll):
                ok = False
                break
        if ok:
            kept_ways.append(way)

    # Directed segments between consecutive OSM nodes.
    # undirected degree for collapse.
    undirected: dict[int, set[int]] = defaultdict(set)
    directed: list[tuple[int, int, int, str]] = []  # src, dst, meters, name
    for way in kept_ways:
        tags = way.get("tags") or {}
        name = tags.get("name") or ""
        refs = way["nodes"]
        dirs = oneway_dirs(tags)
        for a, b in zip(refs, refs[1:]):
            la, loa = nodes_ll[a]
            lb, lob = nodes_ll[b]
            xa, ya = project(la, loa)
            xb, yb = project(lb, lob)
            meters = max(1, int(round(math.hypot(xb - xa, yb - ya))))
            undirected[a].add(b)
            undirected[b].add(a)
            if 1 in dirs:
                directed.append((a, b, meters, name))
            if -1 in dirs:
                directed.append((b, a, meters, name))

    pin_xy = {k: project(*ll) for k, ll in PINS.items()}

    def near_pin(nid: int) -> bool:
        xy = project(*nodes_ll[nid])
        return any(dist2(xy, pxy) <= KEEP_PIN_M2 for pxy in pin_xy.values())

    # Incident names per node (for name-change keep).
    names_at: dict[int, set[str]] = defaultdict(set)
    for src, dst, _m, name in directed:
        if name:
            names_at[src].add(name)
            names_at[dst].add(name)

    keep: set[int] = set()
    for nid, nbrs in undirected.items():
        deg = len(nbrs)
        if deg != 2:
            keep.add(nid)
            continue
        if near_pin(nid):
            keep.add(nid)
            continue
        if len(names_at.get(nid, set())) > 1:
            keep.add(nid)

    # Endpoints of directed graph always keep if degree 1 already handled.
    # Isolated unused OSM nodes ignored.

    # Adjacency for collapse: directed neighbors with (meters, name)
    out_map: dict[int, list[tuple[int, int, str]]] = defaultdict(list)
    for src, dst, meters, name in directed:
        out_map[src].append((dst, meters, name))

    def collapse_from(start: int, first: int) -> tuple[int, int, str] | None:
        """Walk degree-2 chain from start via first. Returns (end, meters, name)."""
        prev = start
        cur = first
        total = 0
        names: list[str] = []
        # find segment start->first
        found = None
        for dst, meters, name in out_map[start]:
            if dst == first:
                found = (meters, name)
                break
        if found is None:
            return None
        total += found[0]
        if found[1]:
            names.append(found[1])
        seen = {start, first}
        while cur not in keep:
            nxts = [d for d, _, _ in out_map[cur] if d != prev]
            if len(nxts) != 1:
                # shouldn't happen if keep is correct; stop at cur
                break
            nxt = nxts[0]
            if nxt in seen:
                return None  # loop
            seg = None
            for d, meters, name in out_map[cur]:
                if d == nxt:
                    seg = (meters, name)
                    break
            if seg is None:
                break
            total += seg[0]
            if seg[1]:
                names.append(seg[1])
            seen.add(nxt)
            prev, cur = cur, nxt
        uniq = {n for n in names if n}
        chain_name = next(iter(uniq)) if len(uniq) == 1 else (names[0] if names else "")
        return cur, max(1, int(round(total))), chain_name

    # For each kept node, follow each outgoing to next kept.
    collapsed: dict[tuple[int, int], tuple[int, str]] = {}
    for src in keep:
        for dst, meters, name in out_map[src]:
            if dst in keep:
                key = (src, dst)
                if key not in collapsed or meters < collapsed[key][0]:
                    nm = name
                    old = collapsed.get(key)
                    if old and old[1] and not nm:
                        nm = old[1]
                    collapsed[key] = (min(meters, collapsed[key][0]) if key in collapsed else meters, nm or (old[1] if old else ""))
                continue
            walked = collapse_from(src, dst)
            if walked is None:
                continue
            end, total, cname = walked
            if end == src:
                continue  # self-loop refuse later
            key = (src, end)
            if key in collapsed:
                prev_m, prev_n = collapsed[key]
                collapsed[key] = (min(prev_m, total), prev_n or cname)
            else:
                collapsed[key] = (total, cname)

    # Drop self-loops.
    collapsed = {k: v for k, v in collapsed.items() if k[0] != k[1]}

    used = set()
    for a, b in collapsed:
        used.add(a)
        used.add(b)

    nodes_out = []
    for nid in sorted(used):
        lat, lon = nodes_ll[nid]
        x, y = project(lat, lon)
        nodes_out.append({"id": f"n:{nid}", "x": x, "y": y})

    edges_out = []
    for (src, dst), (meters, name) in sorted(collapsed.items()):
        rec = {
            "src": f"n:{src}",
            "dst": f"n:{dst}",
            "length_m": int(meters),
        }
        if name:
            rec["name"] = name
        edges_out.append(rec)

    city = {
        "attribution": "© OpenStreetMap contributors",
        "osm_date": "overpass-live",
        "generated": "2026-09-19",
        "bbox": {"south": SOUTH, "north": NORTH, "west": WEST, "east": EAST},
        "origin": {"lat": ORIGIN_LAT, "lon": ORIGIN_LON},
        "nodes": nodes_out,
        "edges": edges_out,
    }
    out_city.write_text(json.dumps(city, separators=(",", ":")) + "\n")
    print(f"nodes={len(nodes_out)} edges={len(edges_out)} -> {out_city}", file=sys.stderr)

    # Snap pins to kept nodes.
    id_xy = {n["id"]: (n["x"], n["y"]) for n in nodes_out}

    def snap(pin: str) -> str | None:
        target = pin_xy[pin]
        best = None
        for nid, xy in id_xy.items():
            d = dist2(xy, target)
            if d > SNAP_POI_M2:
                continue
            if best is None or d < best[0] or (d == best[0] and nid < best[1]):
                best = (d, nid)
        return None if best is None else best[1]

    n_west = snap("west_42")
    n_east = snap("east_42")
    n_park = snap("park_42")
    print(f"snap west={n_west} east={n_east} park={n_park}", file=sys.stderr)
    if not n_west or not n_east:
        print("FAIL: corridor pins missed", file=sys.stderr)
        return 1

    xw, xe = id_xy[n_west][0], id_xy[n_east][0]
    lo, hi = (xw, xe) if xw <= xe else (xe, xw)
    closed = []
    for e in edges_out:
        name = (e.get("name") or "").lower()
        if "42nd street" not in name:
            continue
        xs = id_xy[e["src"]][0]
        xd = id_xy[e["dst"]][0]
        if lo <= xs <= hi and lo <= xd <= hi:
            closed.append(
                {"src": e["src"], "edge_type": "street", "dst": e["dst"]}
            )
    if not (6 <= len(closed) <= 20):
        print(f"WARN closure count {len(closed)} (want 6..20)", file=sys.stderr)
    if len(closed) == 0 or len(closed) > 64:
        print("FAIL: closure count", file=sys.stderr)
        return 1
    out_close.write_text(
        json.dumps(
            {
                "name": "West 42nd Street",
                "between": ["5th Avenue", "8th Avenue"],
                "edges": closed,
            },
            indent=2,
        )
        + "\n"
    )

    def poi(pid, name, kind, pin_key, pinned: str | None):
        node = pinned or snap(pin_key)
        if node is None:
            print(f"drop POI {pid} (pin miss)", file=sys.stderr)
            return None
        return {"id": pid, "name": name, "kind": kind, "node": node}

    pois = [
        poi("poi:battery", "The Battery", "park", "battery", None),
        poi("poi:port-authority", "Port Authority", "transit", "west_42", n_west),
        poi(
            "poi:grand-central",
            "Grand Central Terminal",
            "transit",
            "park_42",
            n_park or n_east,
        ),
        poi("poi:empire", "Empire State Building", "landmark", "empire", None),
        poi("poi:bellevue", "Bellevue Hospital", "hospital", "bellevue", None),
        poi("poi:lincoln", "Lincoln Tunnel portal", "tunnel", "lincoln", None),
    ]
    pois = [p for p in pois if p]
    out_poi.write_text(json.dumps(pois, indent=2) + "\n")
    print(f"closure={len(closed)} pois={len(pois)}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
