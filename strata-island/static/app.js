/* Strata Island console. The map and camera use one RAM snapshot; no polling. */
(() => {
  "use strict";
  const $ = (id) => document.getElementById(id);
  const canvas = $("plat");
  const ctx = canvas.getContext("2d");
  const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");
  const camera = { x: 2800, y: 6100, scale: 0.25 };
  let activeMode = "explore",
    panelCollapsed = false;
  const placeStack = [];
  let placeOpener = null;
  let width = 1,
    height = 1,
    dpr = 1,
    frame = 0,
    animation = 0;
  let city = null,
    subway = null,
    pois = [],
    drawnPois = [],
    meta = null,
    nodes = new Map(),
    streets = [];
  let journey = null,
    mission = null;
  const passport = new Set();
  const addressChoices = new Map();
  let addressImpactCursor = null,
    addressImpactOrigin = null,
    browseAbort = null,
    browseTimer;
  function rememberPlace(p) {
    if (p.category !== "address") return;
    addressChoices.set(p.id, p);
    if (addressChoices.size > 200) {
      for (const id of addressChoices.keys()) {
        if (
          ![selectedPlace?.id, $("from-poi").value, $("to-poi").value].includes(
            id,
          )
        ) {
          addressChoices.delete(id);
          break;
        }
      }
    }
  }
  const missionThemes = {
    icons: {
      name: "Midtown icons",
      ids: ["p:node:4236846689", "p:way:34633854", "p:way:278346578"],
    },
    culture: {
      name: "Museum circuit",
      ids: ["p:relation:3698894", "p:way:170149134", "p:way:388436810"],
    },
    parks: {
      name: "Downtown green spaces",
      ids: ["p:way:22899302", "p:relation:7095444", "p:way:22899286"],
    },
  };
  let pickingStreets = false,
    closureSearchLimit = 6;
  const closureDraft = new Map(),
    closureUndo = [],
    streetGroups = new Map();
  let route = null,
    deskRoute = null,
    desk = null,
    closedStreets = [];
  let travelMode = "car";
  let fromNode = null,
    toNode = null,
    busy = false,
    toastTimer;
  let dark = false,
    ready = false;
  try {
    dark = localStorage.getItem("island-theme") === "dark";
  } catch {
    /* Storage may be disabled. */
  }

  const palettes = {
    light: {
      water: "#dce6e9",
      land: "#e9eae7",
      shore: "#cdd6d4",
      park: "#d1dfce",
      parkText: "#8a9e87",
      road: "#fafbf9",
      roadEdge: "#d4d7d3",
      major: "#ffffff",
      label: "#999f9f",
      district: "#8f9797",
      river: "#99adb5",
      route: "#376ef4",
      routeEdge: "#fff",
      alternate: "#c77a39",
      ink: "#303944",
      card: "#fff",
      shadow: "#1e2e4920",
    },
    dark: {
      water: "#202c35",
      land: "#2d3337",
      shore: "#3a464b",
      park: "#2d403b",
      parkText: "#6d9380",
      road: "#424a50",
      roadEdge: "#272d32",
      major: "#535c64",
      label: "#78828b",
      district: "#77828a",
      river: "#526d7d",
      route: "#729aff",
      routeEdge: "#20262e",
      alternate: "#e9a262",
      ink: "#e9edf3",
      card: "#252b33",
      shadow: "#00000040",
    },
  };
  const palette = () => palettes[dark ? "dark" : "light"];
  const project = ([lat, lon]) => ({
    x: (lon + 74.017) * 111320 * Math.cos((40.7003 * Math.PI) / 180),
    y: (lat - 40.7003) * 110540,
  });
  // Simplified geographic context only. Actual streets and routes use the frozen OSM graph.
  const land = [
    [
      [40.7, -74.017],
      [40.705, -74.02],
      [40.713, -74.017],
      [40.721, -74.013],
      [40.731, -74.01],
      [40.742, -74.009],
      [40.752, -74.005],
      [40.766, -73.994],
      [40.781, -73.986],
      [40.797, -73.977],
      [40.814, -73.966],
      [40.83, -73.95],
      [40.846, -73.945],
      [40.867, -73.933],
      [40.878, -73.922],
      [40.873, -73.91],
      [40.862, -73.919],
      [40.85, -73.929],
      [40.838, -73.935],
      [40.825, -73.934],
      [40.812, -73.933],
      [40.798, -73.929],
      [40.786, -73.939],
      [40.774, -73.943],
      [40.762, -73.955],
      [40.749, -73.968],
      [40.736, -73.974],
      [40.724, -73.972],
      [40.713, -73.976],
      [40.71, -73.986],
      [40.704, -74.003],
    ],
    [
      [40.65, -74.055],
      [40.73, -74.035],
      [40.765, -74.02],
      [40.81, -73.986],
      [40.87, -73.951],
      [40.92, -73.93],
      [40.92, -74.2],
      [40.65, -74.2],
    ],
    [
      [40.67, -74.01],
      [40.696, -73.999],
      [40.705, -73.985],
      [40.712, -73.969],
      [40.723, -73.96],
      [40.735, -73.96],
      [40.746, -73.957],
      [40.755, -73.949],
      [40.768, -73.935],
      [40.781, -73.928],
      [40.8, -73.91],
      [40.88, -73.87],
      [40.88, -73.75],
      [40.65, -73.75],
    ],
    [
      [40.75, -73.961],
      [40.763, -73.951],
      [40.771, -73.942],
      [40.768, -73.943],
      [40.757, -73.954],
    ],
  ].map((p) => p.map(project));
  const parks = [
    [
      [40.7681, -73.9819],
      [40.8006, -73.9582],
      [40.7968, -73.949],
      [40.7644, -73.973],
    ],
    [
      [40.7547, -73.984],
      [40.7556, -73.9819],
      [40.7539, -73.9807],
      [40.7531, -73.9829],
    ],
    [
      [40.7355, -73.9911],
      [40.7375, -73.9897],
      [40.7368, -73.9879],
      [40.7348, -73.9894],
    ],
    [
      [40.7295, -73.9996],
      [40.732, -73.998],
      [40.731, -73.9955],
      [40.7287, -73.9972],
    ],
    [
      [40.7032, -74.019],
      [40.7058, -74.0173],
      [40.7034, -74.0136],
      [40.701, -74.015],
    ],
  ].map((p) => p.map(project));
  const areaLabels = [
    ["MIDTOWN", 40.76, -73.98, "district"],
    ["HELL’S KITCHEN", 40.764, -73.993, "district"],
    ["CHELSEA", 40.745, -74.001, "district"],
    ["MURRAY HILL", 40.745, -73.976, "district"],
    ["GRAMERCY", 40.736, -73.984, "district"],
    ["WEST VILLAGE", 40.734, -74.006, "district"],
    ["SOHO", 40.723, -74.0, "district"],
    ["TRIBECA", 40.716, -74.008, "district"],
    ["UPPER WEST SIDE", 40.79, -73.975, "district"],
    ["UPPER EAST SIDE", 40.778, -73.954, "district"],
    ["LONG ISLAND CITY", 40.751, -73.942, "district"],
    ["GREENPOINT", 40.729, -73.95, "district"],
    ["CENTRAL PARK", 40.782, -73.966, "parkText"],
    ["Hudson River", 40.754, -74.015, "river"],
    ["East River", 40.746, -73.963, "river"],
    ["Bryant Park", 40.7542, -73.9824, "parkText"],
  ].map(([name, lat, lon, kind]) => ({ name, ...project([lat, lon]), kind }));

  function screen(p) {
    return {
      x: width / 2 + (p.x - camera.x) * camera.scale,
      y: height / 2 - (p.y - camera.y) * camera.scale,
    };
  }
  function world(p) {
    return {
      x: camera.x + (p.x - width / 2) / camera.scale,
      y: camera.y - (p.y - height / 2) / camera.scale,
    };
  }
  function eventPoint(e) {
    const r = canvas.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }
  function requestDraw() {
    if (!frame)
      frame = requestAnimationFrame(() => {
        frame = 0;
        draw();
      });
  }
  function resize() {
    const rect = canvas.getBoundingClientRect();
    const changed = width !== rect.width || height !== rect.height;
    width = rect.width;
    height = rect.height;
    dpr = Math.min(devicePixelRatio || 1, 2);
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    if (changed && ready) {
      if (selectedPlace) focusPlace(selectedPlace, false);
      else if (route && activeMode !== "explore") fitRoute(false);
    }
    requestDraw();
  }
  function polygon(points, fill, stroke) {
    ctx.beginPath();
    points.forEach((pt, i) => {
      const p = screen(pt);
      i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y);
    });
    ctx.closePath();
    ctx.fillStyle = fill;
    ctx.fill();
    if (stroke) {
      ctx.strokeStyle = stroke;
      ctx.lineWidth = 1;
      ctx.stroke();
    }
  }
  function drawPath(points, color, lineWidth, dash = []) {
    if (!points || points.length < 2) return;
    ctx.beginPath();
    points.forEach((pt, i) => {
      const p = screen(pt);
      i ? ctx.lineTo(p.x, p.y) : ctx.moveTo(p.x, p.y);
    });
    ctx.strokeStyle = color;
    ctx.lineWidth = lineWidth;
    ctx.setLineDash(dash);
    ctx.stroke();
    ctx.setLineDash([]);
  }
  function roundedRect(x, y, w, h, r, fill) {
    ctx.beginPath();
    ctx.roundRect(x, y, w, h, r);
    ctx.fillStyle = fill;
    ctx.fill();
  }
  function textLabel(text, x, y, size, color, spacing = 0) {
    ctx.font = `500 ${size}px "DM Sans", sans-serif`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    if ("letterSpacing" in ctx) ctx.letterSpacing = `${spacing}px`;
    ctx.fillStyle = color;
    ctx.fillText(text, x, y);
    if ("letterSpacing" in ctx) ctx.letterSpacing = "0px";
  }
  function draw() {
    const p = palette();
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = p.water;
    ctx.fillRect(0, 0, width, height);
    land.forEach((shape) => polygon(shape, p.land, p.shore));
    parks.forEach((shape) => polygon(shape, p.park));
    if (!city) return;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    const visible = [];
    for (const edge of streets) {
      const a = screen(city.nodes[edge.s]),
        b = screen(city.nodes[edge.d]);
      if (
        Math.max(a.x, b.x) < -20 ||
        Math.min(a.x, b.x) > width + 20 ||
        Math.max(a.y, b.y) < -20 ||
        Math.min(a.y, b.y) > height + 20
      )
        continue;
      visible.push({ ...edge, a, b });
    }
    // Batch road strokes, preserving one-way streets as well as bidirectional pairs.
    for (const major of [false, true]) {
      const roadWidth = Math.max(
        major ? 2.4 : 1.2,
        Math.min(major ? 10 : 6, camera.scale * (major ? 21 : 13)),
      );
      for (const casing of [true, false]) {
        ctx.beginPath();
        for (const e of visible) {
          if (e.major !== major) continue;
          ctx.moveTo(e.a.x, e.a.y);
          ctx.lineTo(e.b.x, e.b.y);
        }
        ctx.strokeStyle = casing ? p.roadEdge : major ? p.major : p.road;
        ctx.lineWidth = roadWidth + (casing ? 1.4 : 0);
        ctx.stroke();
      }
    }
    if (route && activeMode === "route" && !selectedPlace) {
      for (const key of ["origin", "destination"]) {
        const endpoint = route[key];
        const anchor = key === "origin" ? route.points[0] : route.points.at(-1);
        if (endpoint && anchor)
          drawPath([endpoint, anchor], p.route || "#527e9a", 2, [4, 5]);
      }
    }
    if (subway && $("show-subway").checked) drawSubway(p);
    const occupied = [];
    for (const a of areaLabels) {
      if (a.name === "Bryant Park" && camera.scale < 0.3) continue;
      const pos = screen(a);
      if (
        pos.x < 50 ||
        pos.x > width - 50 ||
        pos.y < 110 ||
        pos.y > height - 70
      )
        continue;
      if (camera.scale < 0.09 && a.kind === "district") continue;
      const size = a.kind === "river" ? 13 : a.kind === "parkText" ? 10 : 10;
      textLabel(
        a.name,
        pos.x,
        pos.y,
        size,
        p[a.kind],
        a.kind === "district" ? 2 : 1,
      );
      occupied.push({ x: pos.x - 70, y: pos.y - 13, w: 140, h: 26 });
    }
    if ($("show-streets").checked && camera.scale > 0.15) {
      const used = new Set();
      const candidates = visible
        .filter((e) => e.n)
        .sort((a, b) => Number(b.major) - Number(a.major) || b.m - a.m);
      for (const e of candidates) {
        const key = e.n;
        if (used.has(key)) continue;
        const mx = (e.a.x + e.b.x) / 2,
          my = (e.a.y + e.b.y) / 2;
        if (mx < 70 || mx > width - 80 || my < 120 || my > height - 70)
          continue;
        const label = key
          .replace("West ", "W ")
          .replace("East ", "E ")
          .replace("Street", "St")
          .replace("Avenue", "Ave");
        const box = {
          x: mx - label.length * 2.7,
          y: my - 10,
          w: label.length * 5.4,
          h: 20,
        };
        if (
          occupied.some(
            (b) =>
              box.x < b.x + b.w + 10 &&
              box.x + box.w > b.x - 10 &&
              box.y < b.y + b.h + 10 &&
              box.y + box.h > b.y - 10,
          )
        )
          continue;
        let angle = Math.atan2(e.b.y - e.a.y, e.b.x - e.a.x);
        if (angle > Math.PI / 2) angle -= Math.PI;
        if (angle < -Math.PI / 2) angle += Math.PI;
        ctx.save();
        ctx.translate(mx, my);
        ctx.rotate(angle);
        textLabel(label, 0, -2, 9, p.label);
        ctx.restore();
        occupied.push(box);
        used.add(key);
        if (used.size > 30) break;
      }
    }
    const showRoutes = activeMode !== "explore" && !selectedPlace;
    for (const edge of showRoutes ? closedStreets : [])
      drawPath(
        [city.nodes[edge.s], city.nodes[edge.d]],
        p.alternate,
        3,
        [5, 5],
      );
    if (route && showRoutes) {
      if (route.mode === "transit") {
        for (const leg of route.legs) {
          if (!leg.points.length) continue;
          const color =
            leg.mode === "subway" && /^[0-9a-f]{6}$/i.test(leg.color)
              ? `#${leg.color}`
              : p.route;
          drawPath(leg.points, p.routeEdge, 9);
          drawPath(
            leg.points,
            color,
            leg.mode === "subway" ? 5 : 3,
            leg.mode === "walk" ? [3, 6] : [],
          );
          if (leg.mode === "subway")
            for (const point of [leg.points[0], leg.points.at(-1)]) {
              const q = screen(point);
              ctx.beginPath();
              ctx.arc(q.x, q.y, 5, 0, Math.PI * 2);
              ctx.fillStyle = p.card;
              ctx.fill();
              ctx.lineWidth = 2;
              ctx.strokeStyle = color;
              ctx.stroke();
            }
        }
      } else {
        drawPath(route.points, p.routeEdge, 9);
        drawPath(route.points, p.route, 5);
      }
    }
    if (deskRoute && showRoutes) {
      drawPath(deskRoute.points, p.routeEdge, 8);
      drawPath(deskRoute.points, p.alternate, 4);
    }
    if (
      activeMode === "closures" &&
      !selectedPlace &&
      meta?.dataset === "v2" &&
      $("closure-picker").value === "custom"
    ) {
      for (const edge of closureDraft.values()) {
        drawPath([city.nodes[edge.s], city.nodes[edge.d]], p.card, 10);
        drawPath(
          [city.nodes[edge.s], city.nodes[edge.d]],
          p.alternate,
          6,
          [9, 4],
        );
      }
    }
    drawnPois = [];
    if ($("show-pois").checked) {
      const labels = [],
        dots = [];
      const selected = new Set([$("from-poi").value, $("to-poi").value]);
      const priority = (p) =>
        selected.has(p.id)
          ? 0
          : p.aliases?.length
            ? 1
            : p.category !== "landmark"
              ? 2
              : 3;
      const ordered = [...pois].sort((a, b) => priority(a) - priority(b));
      const spacing = camera.scale > 0.7 ? 12 : 24;
      for (const poi of ordered) {
        // Subway stations have their own layer and hit targets.
        if (poi.subway && subway && $("show-subway").checked) continue;
        if (placeCategory && poi.category && poi.category !== placeCategory)
          continue;
        const node = poi.x === undefined ? nodes.get(poi.node) : poi;
        if (!node || node === fromNode || node === toNode) continue;
        const pos = screen(node);
        if (
          pos.x < 20 ||
          pos.x > width - 20 ||
          pos.y < 115 ||
          pos.y > height - 65
        )
          continue;
        if (
          !selected.has(poi.id) &&
          (dots.length >= 180 ||
            dots.some((q) => Math.hypot(q.x - pos.x, q.y - pos.y) < spacing))
        )
          continue;
        dots.push(pos);
        drawnPois.push(poi);
        ctx.beginPath();
        ctx.arc(pos.x, pos.y, 4, 0, Math.PI * 2);
        ctx.fillStyle = p.card;
        ctx.fill();
        ctx.strokeStyle =
          poi.category === "park"
            ? "#689676"
            : poi.category === "transit"
              ? "#688fa9"
              : p.label;
        ctx.lineWidth = 1.5;
        ctx.stroke();
        if (camera.scale > 0.18) {
          ctx.font = '500 10px "DM Sans", sans-serif';
          ctx.textAlign = "left";
          ctx.fillStyle = p.ink;
          const box = {
            x: pos.x + 10,
            y: pos.y - 7,
            w: ctx.measureText(poi.name).width + 8,
            h: 15,
          };
          if (
            labels.length < 60 &&
            !labels.some(
              (b) =>
                box.x < b.x + b.w &&
                box.x + box.w > b.x &&
                box.y < b.y + b.h &&
                box.y + box.h > b.y,
            )
          ) {
            ctx.fillText(poi.name, pos.x + 10, pos.y);
            labels.push(box);
          }
          if (poi.id === $("to-poi").value || poi.id === $("from-poi").value) {
            const anchor = nodes.get(poi.node);
            if (anchor) drawPath([poi, anchor], p.label, 1, [3, 4]);
          }
        }
      }
    }
    if (subway && $("show-subway").checked) drawSubwayStations(p);
    if (journey) {
      const pos = screen(journey.point);
      ctx.beginPath();
      ctx.arc(pos.x, pos.y, 10, 0, Math.PI * 2);
      ctx.fillStyle = p.card;
      ctx.fill();
      ctx.beginPath();
      ctx.arc(pos.x, pos.y, 6, 0, Math.PI * 2);
      ctx.fillStyle = p.route;
      ctx.fill();
    }
    if (showRoutes) {
      marker(fromNode, "A", endpointName("from"), false);
      marker(toNode, "B", endpointName("to"), true);
    }
    if (selectedPlace) {
      const pos = screen(selectedPlace);
      ctx.beginPath();
      ctx.arc(pos.x, pos.y, 25, 0, Math.PI * 2);
      ctx.fillStyle = dark ? "#8cacff30" : "#376ef420";
      ctx.fill();
      ctx.beginPath();
      ctx.arc(pos.x, pos.y, 12, 0, Math.PI * 2);
      ctx.fillStyle = p.route;
      ctx.fill();
      ctx.strokeStyle = p.card;
      ctx.lineWidth = 3;
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(pos.x, pos.y, 3, 0, Math.PI * 2);
      ctx.fillStyle = p.card;
      ctx.fill();
    }
    const target = 90 / camera.scale;
    const distance = [
      10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000,
    ].reduce((a, b) => (Math.abs(b - target) < Math.abs(a - target) ? b : a));
    $("scale-label").textContent =
      distance >= 1000 ? `${distance / 1000} km` : `${distance} m`;
    $("scale-line").style.width = `${distance * camera.scale}px`;
  }
  function subwayColor(id) {
    const color = subway?.routes.find((r) => r.id === id)?.color;
    return color && /^[0-9a-f]{6}$/i.test(color) ? `#${color}` : "#687b93";
  }
  function drawSubway(p) {
    const line = $("subway-line").value,
      seen = new Set();
    ctx.save();
    ctx.globalAlpha = line ? 0.85 : 0.42;
    for (const edge of subway.edges) {
      const transfer = edge.kind === "transfer",
        service = edge.kind.slice(5);
      if (line && !transfer && service !== line) continue;
      if (transfer && line) continue;
      const key = [edge.source, edge.target].sort().join("|");
      if (seen.has(key)) continue;
      seen.add(key);
      drawPath(
        [subway.byId.get(edge.source), subway.byId.get(edge.target)],
        transfer ? p.ink : subwayColor(service),
        transfer ? 1.5 : line ? 3.5 : 2.5,
        transfer ? [3, 4] : [],
      );
    }
    ctx.restore();
  }
  function drawSubwayStations(p) {
    const line = $("subway-line").value;
    const serving = line
      ? new Set(
          subway.edges
            .filter((e) => e.kind === `ride:${line}`)
            .flatMap((e) => [e.source, e.target]),
        )
      : null;
    const labels = [];
    for (const station of subway.stations) {
      if (serving && !serving.has(station.id)) continue;
      if (placeCategory && placeCategory !== "transit") continue;
      const pos = screen(station);
      if (
        pos.x < 15 ||
        pos.x > width - 15 ||
        pos.y < 115 ||
        pos.y > height - 65
      )
        continue;
      const place = pois.find((p) => p.id === station.id);
      drawnPois.unshift(place);
      ctx.beginPath();
      ctx.arc(pos.x, pos.y, 5, 0, Math.PI * 2);
      ctx.fillStyle = p.card;
      ctx.fill();
      ctx.lineWidth = 2;
      ctx.strokeStyle = line ? subwayColor(line) : p.ink;
      ctx.stroke();
      if (
        camera.scale > 0.24 &&
        !labels.some(
          (q) => Math.abs(q.y - pos.y) < 18 && Math.abs(q.x - pos.x) < 155,
        )
      ) {
        ctx.font = '600 10px "DM Sans", sans-serif';
        ctx.textAlign = "left";
        ctx.textBaseline = "middle";
        ctx.lineWidth = 4;
        ctx.strokeStyle = p.card;
        ctx.strokeText(station.name, pos.x + 10, pos.y);
        ctx.fillStyle = p.ink;
        ctx.fillText(station.name, pos.x + 10, pos.y);
        labels.push(pos);
      }
    }
  }
  function marker(node, letter, label, destination) {
    if (!node) return;
    const pos = screen(node),
      p = palette();
    if (
      pos.x < -100 ||
      pos.x > width + 100 ||
      pos.y < -80 ||
      pos.y > height + 80
    )
      return;
    ctx.save();
    ctx.shadowColor = p.shadow;
    ctx.shadowBlur = 14;
    ctx.shadowOffsetY = 4;
    ctx.beginPath();
    ctx.arc(pos.x, pos.y, 13, 0, Math.PI * 2);
    ctx.fillStyle = destination ? p.route : p.ink;
    ctx.fill();
    ctx.shadowColor = "transparent";
    ctx.lineWidth = 3;
    ctx.strokeStyle = p.card;
    ctx.stroke();
    textLabel(
      letter,
      pos.x,
      pos.y + 0.5,
      10,
      destination ? "#fff" : dark ? "#22262d" : "#fff",
    );
    if (camera.scale > 0.1) {
      ctx.font = '550 11px "DM Sans", sans-serif';
      const w = Math.min(220, ctx.measureText(label).width + 24);
      const x = Math.max(8, Math.min(width - w - 8, pos.x - w / 2));
      const y = destination ? pos.y - 54 : pos.y + 25;
      ctx.shadowColor = p.shadow;
      ctx.shadowBlur = 12;
      ctx.shadowOffsetY = 3;
      roundedRect(x, y, w, 29, 7, p.card);
      ctx.shadowColor = "transparent";
      textLabel(label, x + w / 2, y + 14.5, 11, p.ink);
    }
    ctx.restore();
  }
  function moveCamera(target, animate = true) {
    cancelAnimationFrame(animation);
    if (!animate || reducedMotion.matches) {
      Object.assign(camera, target);
      requestDraw();
      return;
    }
    const start = { ...camera },
      time = performance.now();
    function step(now) {
      const t = Math.min(1, (now - time) / 450),
        ease = 1 - Math.pow(1 - t, 3);
      for (const key of ["x", "y", "scale"])
        camera[key] = start[key] + (target[key] - start[key]) * ease;
      requestDraw();
      if (t < 1) animation = requestAnimationFrame(step);
    }
    animation = requestAnimationFrame(step);
  }
  // Fit locations into the visible map, accounting for the floating panel.
  function mapViewport() {
    const panel = $("activity-panel").getBoundingClientRect();
    return innerWidth <= 760
      ? {
          left: 20,
          right: Math.max(120, width - 70),
          top: 24,
          bottom: Math.max(160, panel.top - 24),
        }
      : {
          left: panel.right + 36,
          right: width - 86,
          top: 32,
          bottom: height - 48,
        };
  }
  function centeredCamera(point, scale) {
    const view = mapViewport();
    return {
      x: point.x + (width / 2 - (view.left + view.right) / 2) / scale,
      y: point.y + ((view.top + view.bottom) / 2 - height / 2) / scale,
      scale,
    };
  }
  function focusPlace(place, animate = true) {
    moveCamera(centeredCamera(place, Math.max(0.7, camera.scale)), animate);
  }
  function fitPoints(points, overview = false, animate = true) {
    if (!points.length) return;
    let minX = Infinity,
      maxX = -Infinity,
      minY = Infinity,
      maxY = -Infinity;
    for (const p of points) {
      minX = Math.min(minX, p.x);
      maxX = Math.max(maxX, p.x);
      minY = Math.min(minY, p.y);
      maxY = Math.max(maxY, p.y);
    }
    const view = mapViewport();
    const spanX = Math.max(overview ? 1 : 1800, maxX - minX),
      spanY = Math.max(overview ? 1 : 1800, maxY - minY);
    const scale = Math.min(
      2,
      Math.max(
        0.015,
        Math.min(
          Math.max(100, view.right - view.left - 80) / spanX,
          Math.max(100, view.bottom - view.top - 80) / spanY,
        ),
      ),
    );
    moveCamera(
      centeredCamera({ x: (minX + maxX) / 2, y: (minY + maxY) / 2 }, scale),
      animate,
    );
  }
  function fitRoute(animate = true) {
    if (route)
      fitPoints(
        [...route.points, ...(deskRoute?.points || [])],
        false,
        animate,
      );
    else if (city) fitPoints(city.nodes, true, animate);
  }
  function zoomAt(factor, at = { x: width / 2, y: height / 2 }) {
    cancelAnimationFrame(animation);
    const before = world(at);
    camera.scale = Math.min(8, Math.max(0.015, camera.scale * factor));
    const after = world(at);
    camera.x += before.x - after.x;
    camera.y += before.y - after.y;
    requestDraw();
  }
  function endpointName(which) {
    const select = $(`${which}-poi`);
    return select.selectedOptions[0]?.textContent || "Map intersection";
  }
  function selectedNode(which) {
    const token = $(`${which}-poi`).value;
    if (travelMode === "transit") {
      const place =
        addressChoices.get(token) || pois.find((p) => p.id === token);
      if (place) return place;
    }
    return (
      nodes.get(
        addressChoices.get(token)?.node ||
          pois.find((p) => p.id === token)?.node ||
          token,
      ) || null
    );
  }
  function clearRoutes() {
    route = null;
    deskRoute = null;
    $("journey-legs").hidden = true;
    $("journey-legs").replaceChildren();
    $("journey-note").hidden = true;
    $("route-distance").textContent = "—";
    $("route-unit").textContent = "km";
    $("route-tag").textContent = "READY TO EXPLORE";
    $("route-description").textContent =
      "Choose your places, then find a route.";
    $("desk-distance").textContent = "—";
    $("route-delta").textContent = "—";
    $("route-end").querySelector("span").textContent = endpointName("to");
    renderMission();
    requestDraw();
  }
  function readEndpoints() {
    fromNode = selectedNode("from");
    toNode = selectedNode("to");
    for (const which of ["from", "to"])
      if ($(`${which}-search`))
        $(`${which}-search`).value = endpointName(which);
    placesRequest++;
    if (ready && meta?.dataset === "v2") browsePlaces();
    clearRoutes();
  }
  function formatDistance(m) {
    return m >= 1000 ? `${(m / 1000).toFixed(2)} km` : `${m} m`;
  }
  function renderRoutes() {
    if (!route) return;
    $("route-distance").textContent =
      route.length_m >= 1000
        ? (route.length_m / 1000).toFixed(2)
        : String(route.length_m);
    $("route-unit").textContent = route.length_m >= 1000 ? "km" : "m";
    $("route-tag").textContent = "ROUTE READY";
    $("route-description").textContent = route.used_closed
      ? "Via West 42nd Street · Official city"
      : "Shortest path · Official city";
    $("route-end").querySelector("span").textContent = endpointName("to");
    if (route.mode === "transit") {
      $("route-distance").textContent = Math.max(
        1,
        Math.ceil(route.duration_s / 60),
      );
      $("route-unit").textContent = "min est.";
      $("route-description").textContent = route.boardings
        ? `${formatDistance(route.walking_m)} walking · ${route.transfers ? `${route.transfers} transfer${route.transfers > 1 ? "s" : ""}` : "No transfers"}`
        : `Walk ${formatDistance(route.walking_m)} · Faster than taking the subway`;
      $("journey-note").hidden = false;
      $("journey-note").textContent =
        "Typical weekday service · No live arrivals. Walking access is approximate." +
        (desk ? " Car closures do not change this trip." : "");
      $("journey-legs").hidden = false;
      $("journey-legs").replaceChildren(
        ...route.legs.map((leg, i) => {
          const li = document.createElement("li"),
            button = document.createElement("button"),
            badge = document.createElement("span"),
            content = document.createElement("span"),
            title = document.createElement("strong"),
            detail = document.createElement("small");
          button.type = "button";
          badge.className = "journey-badge";
          badge.textContent = leg.mode === "subway" ? leg.route : "↗";
          if (leg.mode === "subway" && /^[0-9a-f]{6}$/i.test(leg.color)) {
            badge.style.background = `#${leg.color}`;
            badge.classList.add("train");
          }
          title.textContent =
            leg.mode === "subway"
              ? `${leg.from} → ${leg.to}`
              : `Walk ${formatDistance(leg.meters)}`;
          detail.textContent =
            leg.mode === "subway"
              ? `Toward ${leg.headsign} · ${leg.stops} stop${leg.stops === 1 ? "" : "s"} · ${Math.ceil(leg.seconds / 60)} min including boarding`
              : `${Math.max(1, Math.ceil(leg.seconds / 60))} min${route.legs[i + 1]?.mode === "subway" ? ` to ${route.legs[i + 1].from}` : i === route.legs.length - 1 ? ` to ${endpointName("to")}` : " connection"}`;
          content.append(title, detail);
          button.append(badge, content);
          button.onclick = () => fitPoints(leg.points, false, true);
          li.append(button);
          return li;
        }),
      );
      if (desk) {
        $("desk-distance").textContent = "Unchanged";
        $("route-delta").textContent = "Car closures only";
      }
    }
    if (deskRoute) {
      $("desk-distance").textContent = formatDistance(deskRoute.length_m);
      const delta = deskRoute.length_m - route.length_m;
      $("route-delta").textContent =
        delta === 0 ? "Same distance" : `+${formatDistance(delta)}`;
    }
    updateActivity();
    requestDraw();
  }
  function setStatus(
    message,
    { error = false, loading = false, persistent = false } = {},
  ) {
    clearTimeout(toastTimer);
    const el = $("route-status");
    el.replaceChildren();
    if (loading) {
      const spinner = document.createElement("span");
      spinner.className = "loading-spinner";
      el.append(spinner);
    }
    el.append(document.createTextNode(message));
    el.classList.remove("quiet");
    el.classList.toggle("error", error);
    if (!persistent && !loading && !error)
      toastTimer = setTimeout(() => el.classList.add("quiet"), 2200);
  }
  const messages = {
    "resource_exhausted.island.analysis":
      "The city is busy. Try again in a moment.",
    "failed_precondition.island.version":
      "This scenario changed. Reload its history and try again.",
    "failed_precondition.island.walk_anchor":
      "No connected walking path was found near this location. Try a nearby station or place.",
    "failed_precondition.island.place_anchor":
      "This destination does not have a routing connection yet.",
    "failed_precondition.island.desk_cap":
      "All scenario spaces are in use. Archive a scenario first.",
    "failed_precondition.island.unreachable":
      "No connecting route was found. Try another destination.",
    "invalid_argument.island.snap":
      "Choose a point closer to a street intersection.",
    "invalid_argument.island.branch":
      "That scenario is no longer available. Refresh to load the latest city.",
  };
  async function api(path, body, signal) {
    let creationKey = null;
    if (body && ["scenarios", "close"].includes(path)) {
      const fingerprint = JSON.stringify({ path, body });
      let pending;
      try {
        pending = JSON.parse(
          sessionStorage.getItem("island-pending-closure") || "null",
        );
      } catch {}
      creationKey =
        pending?.fingerprint === fingerprint ? pending.id : crypto.randomUUID();
      try {
        sessionStorage.setItem(
          "island-pending-closure",
          JSON.stringify({ fingerprint, id: creationKey }),
        );
      } catch {}
      body = { ...body, request_id: creationKey };
    }
    let response;
    try {
      response = await fetch(
        `/api/${path}`,
        body === undefined
          ? { signal }
          : {
              signal,
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify(body),
            },
      );
    } catch (error) {
      if (error.name === "AbortError") throw error;
      throw new Error(
        "Unable to reach the city. Check the connection and try again.",
      );
    }
    if (!response.ok) {
      let data;
      try {
        data = await response.json();
      } catch {
        data = {};
      }
      throw new Error(
        messages[data.code] ||
          `The request could not be completed${data.code ? ` (${data.code})` : ""}.`,
      );
    }
    const result = await response.json();
    if (creationKey) {
      try {
        sessionStorage.removeItem("island-pending-closure");
      } catch {}
    }
    return result;
  }
  function syncControls() {
    for (const id of [
      "from-poi",
      "to-poi",
      "btn-swap",
      "btn-route",
      "desk-select",
      "mode-car",
      "mode-transit",
    ])
      $(id).disabled = busy || !ready;
    $("btn-close").disabled =
      busy ||
      !ready ||
      Boolean(meta && meta.branch_count >= meta.live_cap) ||
      (meta?.dataset === "v2" &&
        $("closure-picker").value === "custom" &&
        !closureDraft.size);
    for (const id of [
      "closure-picker",
      "closure-name",
      "closure-search",
      "btn-pick-streets",
    ])
      $(id).disabled = busy || !ready;
    $("closure-undo").disabled = busy || !closureUndo.length;
    $("closure-clear").disabled = busy || !closureDraft.size;
    for (const button of document.querySelectorAll(
      "#closure-search-results button, #closure-selection button",
    ))
      button.disabled = busy;
    $("btn-compare").disabled = busy || !desk;
    $("btn-archive").disabled = busy || !desk;
    $("btn-audit").disabled = busy || !ready;
    document.body.classList.toggle("busy", busy);
    $("route-form").setAttribute("aria-busy", String(busy));
    for (const id of ["from-search", "to-search"])
      if ($(id)) $(id).disabled = busy || !ready;
    $("mission-start").disabled = busy || !ready;
    $("mission-theme").disabled = busy || !ready;
    $("mission-plan").disabled =
      busy || !mission || mission.visited.length === 3;
    $("mission-travel").disabled = busy || !missionCanTravel();
    if (ready && selectedPlace) {
      $("place-start").disabled =
        busy || (travelMode !== "transit" && !selectedPlace.node);
      $("place-route").disabled =
        busy || (travelMode !== "transit" && !selectedPlace.node);
    }
  }
  async function task(message, work) {
    if (busy) return;
    busy = true;
    syncControls();
    setStatus(message, { loading: true });
    try {
      await work();
    } catch (error) {
      setStatus(error.message, { error: true });
    } finally {
      busy = false;
      if ($("route-status").querySelector(".loading-spinner"))
        $("route-status").classList.add("quiet");
      syncControls();
    }
  }
  function scenarioName(name) {
    return `Scenario ${Number(name.replace("desk-", ""))}`;
  }
  function renderMeta(data) {
    meta = data;
    const desks = data.branches
      .filter((b) => b.name.startsWith("desk-"))
      .sort((a, b) => Number(a.name.slice(5)) - Number(b.name.slice(5)));
    if (!desks.some((b) => b.name === desk)) desk = desks.at(-1)?.name || null;
    $("desk-select").replaceChildren(
      ...desks.map((b) => new Option(scenarioName(b.name), b.name)),
    );
    if (desk) $("desk-select").value = desk;
    $("desk-controls").hidden = !desk;
    $("desk-legend").hidden = !desk;
    $("closure-tag").textContent = desk ? "CLOSED" : "OPEN";
    $("closure-tag").classList.toggle("closed", Boolean(desk));
    $("map-mode-label").textContent = desk
      ? `City + ${scenarioName(desk).toLowerCase()}`
      : "Official city";
    $("scenario-description").textContent = desk
      ? "The official city stays intact. Your scenario takes a different way."
      : "Close this stretch in a separate scenario and discover the way around.";
    $("btn-close").querySelector("span").textContent = desk
      ? "Create another scenario"
      : "Create closure scenario";
    $("scenario-limit").hidden = data.branch_count < data.live_cap;
    $("nodes").textContent = city.nodes.length.toLocaleString();
    $("edges").textContent = city.edges.length.toLocaleString();
    $("storage").textContent = data.durable ? "Persistent" : "In memory";
    $("persist").textContent = `${data.persist_ms} ms`;
    $("findings-count").textContent = data.findings.length;
    $("findings-list").replaceChildren(
      ...data.findings.map((f) => {
        const li = document.createElement("li"),
          strong = document.createElement("strong");
        strong.textContent = f.title;
        li.append(strong, document.createTextNode(f.detail));
        return li;
      }),
    );
    updateClosureEditor();
    syncControls();
  }
  async function refreshMeta() {
    renderMeta(await api("meta"));
  }
  async function loadClosure() {
    placesRequest++;
    closedStreets = [];
    if (meta?.dataset === "v2") {
      await loadScenarioHistory();
      browsePlaces();
    }
    if (!desk) {
      requestDraw();
      return;
    }
    const snapshot = await api(`city?branch=${encodeURIComponent(desk)}`);
    const pairs = new Set(
      snapshot.edges.map(
        (e) => `${snapshot.nodes[e.s].id}|${snapshot.nodes[e.d].id}`,
      ),
    );
    const seen = new Set();
    for (const e of city.edges) {
      if (pairs.has(`${city.nodes[e.s].id}|${city.nodes[e.d].id}`)) continue;
      const key = [e.s, e.d].sort((a, b) => a - b).join(":");
      if (seen.has(key)) continue;
      seen.add(key);
      closedStreets.push(e);
    }
    requestDraw();
  }
  async function routePair(fit = true) {
    clearRoutes();
    if (!fromNode || !toNode) {
      setStatus("Choose both a starting point and destination.");
      return;
    }
    const endpoints = {
      from: $("from-poi").value,
      to: $("to-poi").value,
      mode: travelMode,
    };
    route = await api("route", {
      branch: "city",
      version: meta?.versions?.city,
      ...endpoints,
    });
    let alternateError = null;
    if (desk && travelMode === "car") {
      try {
        deskRoute = await api("route", {
          branch: desk,
          version: meta?.versions?.[desk],
          ...endpoints,
        });
      } catch (error) {
        alternateError = error;
        $("desk-distance").textContent = "Unavailable";
      }
    }
    renderRoutes();
    renderMission();
    if (meta?.dataset === "v2") updateClosureEditor();
    if (fit && !selectedPlace) fitRoute();
    if (alternateError)
      setStatus(`Official route ready. Scenario: ${alternateError.message}`, {
        error: true,
      });
    else $("route-status").classList.add("quiet");
  }
  function missionCanTravel() {
    return Boolean(
      mission?.target &&
      !mission.visited.includes(mission.target) &&
      $("from-poi").value === mission.origin &&
      $("to-poi").value === mission.target &&
      (desk ? deskRoute : route),
    );
  }
  function saveMission() {
    try {
      localStorage.setItem(
        "island-passport-v1",
        JSON.stringify({ mission, passport: [...passport] }),
      );
    } catch {
      /* Private browsing can still play this session. */
    }
  }
  function restoreMission() {
    try {
      const saved = JSON.parse(
        localStorage.getItem("island-passport-v1") || "null",
      );
      const validIds = new Set(
        Object.values(missionThemes).flatMap((theme) => theme.ids),
      );
      if (Array.isArray(saved?.passport))
        for (const id of saved.passport) if (validIds.has(id)) passport.add(id);
      const state = saved?.mission,
        theme = missionThemes[state?.theme];
      if (
        theme &&
        Array.isArray(state.visited) &&
        state.visited.every((id) => theme.ids.includes(id)) &&
        (nodes.has(state.origin) ||
          pois.some((p) => p.id === state.origin && p.node))
      ) {
        mission = {
          theme: state.theme,
          visited: [...new Set(state.visited)],
          origin: state.origin,
          target:
            theme.ids.includes(state.target) &&
            !state.visited.includes(state.target)
              ? state.target
              : null,
          meters:
            Number.isFinite(state.meters) && state.meters >= 0
              ? Math.min(state.meters, 1e7)
              : 0,
        };
        $("mission-theme").value = mission.theme;
      }
    } catch {
      /* Ignore malformed saved progress. */
    }
    renderMission();
  }
  function renderMission() {
    if (!$("mission-panel")) return;
    $("passport-count").textContent = `${passport.size} VISITED`;
    $("mission-badges").replaceChildren(
      ...Object.values(missionThemes)
        .filter((theme) => theme.ids.every((id) => passport.has(id)))
        .map((theme) => {
          const badge = document.createElement("span");
          badge.className = "mission-badge";
          badge.textContent = `✦ ${theme.name}`;
          return badge;
        }),
    );
    const theme = missionThemes[mission?.theme || $("mission-theme").value];
    $("mission-stops").replaceChildren(
      ...theme.ids.map((id) => {
        const item = document.createElement("li"),
          place = pois.find((p) => p.id === id);
        item.textContent = `${mission?.visited.includes(id) ? "✓ " : ""}${place?.name || "Landmark"}${mission?.target === id ? " · next stop" : ""}`;
        item.classList.toggle(
          "visited",
          Boolean(mission?.visited.includes(id)),
        );
        return item;
      }),
    );
    const count = mission?.visited.length || 0;
    $("mission-progress").value = count;
    $("mission-start").textContent = mission
      ? "Start a new mission"
      : "Start mission";
    $("mission-plan").hidden = !mission || count === 3;
    $("mission-travel").hidden = !mission || count === 3;
    $("mission-travel").disabled = busy || !missionCanTravel();
    if (!journey)
      $("mission-status").textContent = !mission
        ? "Start from your route's starting point."
        : count === 3
          ? `Mission complete! ${theme.name} badge earned · ${formatDistance(mission.meters)} explored.`
          : `${count} / 3 stops visited. ${missionCanTravel() ? "Route ready—travel to collect this stop." : "Route to the next reachable stop to continue."}`;
  }
  async function planMissionLeg() {
    if (travelMode !== "car") changeTravelMode("car");
    if (!mission || mission.visited.length === 3) return;
    const data = await api("discover", {
      origin: mission.origin,
      branch: desk || "city",
      max_m: 100000,
    });
    const remaining = missionThemes[mission.theme].ids.filter(
      (id) => !mission.visited.includes(id),
    );
    const next = data.results.find((r) => remaining.includes(r.place.id));
    if (!next) {
      mission.target = null;
      saveMission();
      renderMission();
      $("mission-status").textContent =
        "The remaining stops are unreachable in this scenario. Reopen the corridor or choose another scenario, then try again.";
      return;
    }
    mission.target = next.place.id;
    const origin = pois.find((p) => p.id === mission.origin);
    setEndpoint(
      "from",
      mission.origin,
      origin?.name || "Starting intersection",
    );
    setEndpoint("to", next.place.id, next.place.name);
    readEndpoints();
    saveMission();
    await routePair();
    renderMission();
  }
  $("mission-theme").onchange = () => {
    if (!mission) renderMission();
  };
  $("mission-start").onclick = () =>
    task("Finding your first landmark…", async () => {
      if (!fromNode) throw new Error("Choose a starting point first.");
      mission = {
        theme: $("mission-theme").value,
        visited: [],
        origin: $("from-poi").value,
        target: null,
        meters: 0,
      };
      setPickingStreets(false);
      saveMission();
      await planMissionLeg();
    });
  $("mission-plan").onclick = () =>
    task("Finding your next landmark…", planMissionLeg);
  $("mission-travel").onclick = () => {
    if (!missionCanTravel() || busy) return;
    task("Exploring the route…", async () => {
      const path = desk ? deskRoute : route,
        target = mission.target;
      const distances = [0];
      for (let i = 1; i < path.points.length; i++)
        distances.push(
          distances[i - 1] +
            Math.hypot(
              path.points[i].x - path.points[i - 1].x,
              path.points[i].y - path.points[i - 1].y,
            ),
        );
      const total = distances.at(-1),
        duration = reducedMotion.matches
          ? 0
          : Math.min(6500, Math.max(2200, path.length_m));
      await new Promise((resolve) => {
        const start = performance.now();
        function tick(now) {
          const progress = duration ? Math.min(1, (now - start) / duration) : 1;
          const distance = total * progress;
          let i = 1;
          while (i < distances.length - 1 && distances[i] < distance) i++;
          const a = path.points[Math.max(0, i - 1)],
            b = path.points[Math.min(i, path.points.length - 1)];
          const fraction =
            distances[i] > distances[i - 1]
              ? (distance - distances[i - 1]) /
                (distances[i] - distances[i - 1])
              : 1;
          journey = {
            point: {
              x: a.x + (b.x - a.x) * fraction,
              y: a.y + (b.y - a.y) * fraction,
            },
          };
          $("mission-status").textContent =
            `Traveling to ${pois.find((p) => p.id === target).name} · ${Math.round(progress * 100)}%`;
          requestDraw();
          if (progress < 1) requestAnimationFrame(tick);
          else resolve();
        }
        requestAnimationFrame(tick);
      });
      journey = null;
      mission.visited.push(target);
      passport.add(target);
      mission.origin = target;
      mission.target = null;
      mission.meters += path.length_m;
      saveMission();
      renderMission();
      requestDraw();
      setStatus(
        mission.visited.length === 3
          ? "Mission complete. Your explorer badge is earned!"
          : "Landmark discovered! Choose Route to next stop to keep exploring.",
      );
    });
  };

  function streetKey(edge) {
    return [edge.s, edge.d].sort((a, b) => a - b).join(":");
  }
  function makeStreetIndex() {
    streetGroups.clear();
    const junctionNames = new Map();
    for (const e of city.edges) {
      for (const id of [e.s, e.d]) {
        if (!junctionNames.has(id)) junctionNames.set(id, new Set());
        if (e.n) junctionNames.get(id).add(e.n);
      }
      const key = streetKey(e);
      if (streetGroups.has(key)) streetGroups.get(key).directions.push(e);
      else
        streetGroups.set(key, {
          ...e,
          key,
          directions: [e],
          major:
            /Avenue|Broadway|Drive|Highway|Parkway|42nd Street|57th Street|34th Street|14th Street/i.test(
              e.n || "",
            ),
        });
    }
    streets = [...streetGroups.values()];
    for (const e of streets) {
      const cross = (id) =>
        [...junctionNames.get(id)].filter((name) => name !== e.n).sort()[0];
      const a = cross(e.s),
        b = cross(e.d);
      const start = city.nodes[e.s],
        end = city.nodes[e.d];
      e.section =
        a && b
          ? `${a} → ${b}`
          : a || b
            ? `Near ${a || b}`
            : `Map section ${e.key}`;
      e.section += ` · ${Math.round(Math.hypot(end.x - start.x, end.y - start.y))} m`;
    }
  }
  function draftEdges() {
    return [...closureDraft.values()].flatMap((e) =>
      e.directions.map((d) => ({
        src: city.nodes[d.s].id,
        dst: city.nodes[d.d].id,
        edge_type: "street",
      })),
    );
  }
  function setPickingStreets(active) {
    const wasPicking = pickingStreets;
    pickingStreets = active;
    if (wasPicking && !active) $("route-status").classList.add("quiet");
    canvas.classList.toggle("picking-streets", active);
    $("btn-pick-streets").setAttribute("aria-pressed", String(active));
    $("btn-pick-streets").textContent = active
      ? "Done selecting streets"
      : "Select streets on map";
    if (active) {
      setStatus(
        "Closure editor: click street sections to toggle them. Drag to pan; Escape to finish.",
        { persistent: true },
      );
      if (innerWidth <= 760)
        canvas.scrollIntoView({ block: "center", behavior: "smooth" });
      canvas.focus({ preventScroll: true });
    }
  }
  function toggleClosureStreet(edge) {
    if (busy) return;
    if (
      !closureDraft.has(edge.key) &&
      draftEdges().length + edge.directions.length > 200
    ) {
      setStatus(
        "This selection is full. Remove a street section before adding another.",
        { error: true },
      );
      return;
    }
    closureUndo.push([...closureDraft.keys()]);
    if (closureUndo.length > 100) closureUndo.shift();
    if (closureDraft.has(edge.key)) closureDraft.delete(edge.key);
    else closureDraft.set(edge.key, edge);
    updateClosureEditor();
  }
  function renderClosureSearch() {
    const query = $("closure-search").value.toLowerCase().trim();
    const routeKeys = new Set(
      route?.nodes.slice(0, -1).map((id, i) => `${id}|${route.nodes[i + 1]}`) ||
        [],
    );
    const terms = query.split(/\s+/).filter(Boolean);
    const matchesWords = (text) =>
      terms.every((term) =>
        text
          .toLowerCase()
          .split(/[^\p{L}\p{N}]+/u)
          .some((word) => word.startsWith(term)),
      );
    const matches = streets.filter((e) =>
      query
        ? matchesWords(`${e.n || ""} ${e.section}`)
        : e.directions.some((d) =>
            routeKeys.has(`${city.nodes[d.s].id}|${city.nodes[d.d].id}`),
          ),
    );
    if (query)
      matches.sort(
        (a, b) =>
          Number(matchesWords(b.n || "")) - Number(matchesWords(a.n || "")) ||
          (a.n || "").localeCompare(b.n || "") ||
          a.key.localeCompare(b.key),
      );
    $("closure-search-results").replaceChildren(
      ...matches.slice(0, closureSearchLimit).map((e) => {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "closure-section";
        button.dataset.streetKey = e.key;
        button.setAttribute("aria-pressed", String(closureDraft.has(e.key)));
        const title = document.createElement("strong"),
          subtitle = document.createElement("span");
        title.textContent = e.n || "Unnamed street";
        subtitle.textContent = e.section;
        button.append(title, subtitle);
        button.onclick = () => {
          toggleClosureStreet(e);
          fitPoints([city.nodes[e.s], city.nodes[e.d]], false);
        };
        return button;
      }),
    );
    if (!matches.length)
      $("closure-search-results").textContent = query
        ? "No matching street sections."
        : "Find a route or search for a street.";
    $("closure-search-more").hidden = matches.length <= closureSearchLimit;
  }
  function updateClosureEditor() {
    const custom =
      meta?.dataset === "v2" && $("closure-picker").value === "custom";
    $("closure-editor").hidden = !custom;
    if (!custom) setPickingStreets(false);
    $("closure-selection-status").textContent = closureDraft.size
      ? `${closureDraft.size} street section${closureDraft.size === 1 ? "" : "s"} selected · both directions where available`
      : "No street sections selected.";
    $("closure-selection").replaceChildren(
      ...[...closureDraft.values()].map((e) => {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "text-button";
        button.textContent = `× ${e.n || "Unnamed street"} · ${e.section}`;
        button.setAttribute(
          "aria-label",
          `Remove ${e.n || "street"}, ${e.section}`,
        );
        button.onclick = () => toggleClosureStreet(e);
        return button;
      }),
    );
    if (custom) renderClosureSearch();
    if (!desk) {
      document.querySelector(".closure-street strong").textContent = custom
        ? "Your street selection"
        : $("closure-picker").value === "route"
          ? "Current route"
          : "West 42nd Street";
      document.querySelector(".closure-street div > span").textContent = custom
        ? "Preview in orange on the map"
        : $("closure-picker").value === "route"
          ? "Close the route's travel direction"
          : "5th Avenue → 8th Avenue";
    }
    syncControls();
    requestDraw();
  }
  $("closure-picker").onchange = updateClosureEditor;
  $("btn-pick-streets").onclick = () => setPickingStreets(!pickingStreets);
  $("closure-search").oninput = () => {
    closureSearchLimit = 6;
    renderClosureSearch();
  };
  $("closure-search-more").onclick = () => {
    closureSearchLimit += 6;
    renderClosureSearch();
  };
  $("closure-clear").onclick = () => {
    closureUndo.push([...closureDraft.keys()]);
    closureDraft.clear();
    updateClosureEditor();
  };
  $("closure-undo").onclick = () => {
    const previous = closureUndo.pop();
    if (!previous) return;
    closureDraft.clear();
    for (const key of previous) closureDraft.set(key, streetGroups.get(key));
    updateClosureEditor();
  };

  function changeTravelMode(mode) {
    travelMode = mode;
    $("mode-car").setAttribute("aria-pressed", String(mode === "car"));
    $("mode-transit").setAttribute("aria-pressed", String(mode === "transit"));
    readEndpoints();
    syncControls();
  }
  for (const mode of ["car", "transit"])
    $("mode-" + mode).onclick = () => {
      if (busy || mode === travelMode) return;
      changeTravelMode(mode);
      task("Finding your route…", () => routePair());
    };
  $("route-form").addEventListener("submit", (e) => {
    e.preventDefault();
    task("Finding your route…", () => routePair());
  });
  for (const id of ["from-poi", "to-poi"])
    $(id).addEventListener("change", readEndpoints);
  $("btn-swap").addEventListener("click", () => {
    const a = $("from-poi"),
      b = $("to-poi"),
      av = a.value,
      bv = b.value,
      an = endpointName("from"),
      bn = endpointName("to");
    setEndpoint("from", bv, bn);
    setEndpoint("to", av, an);
    readEndpoints();
    task("Finding the return route…", () => routePair());
  });
  $("btn-close").addEventListener("click", () =>
    task("Creating your closure scenario…", async () => {
      let result;
      if (meta?.dataset === "v2" && $("closure-picker").value === "custom") {
        if (!closureDraft.size)
          throw new Error("Select at least one street section.");
        const names = [
          ...new Set(
            [...closureDraft.values()].map((e) => e.n || "Unnamed street"),
          ),
        ];
        const name =
          $("closure-name").value.trim() ||
          `${names[0]}${names.length > 1 ? ` + ${names.length - 1} more streets` : " closure"}`;
        if (new TextEncoder().encode(name).length > 120)
          throw new Error("Use a shorter scenario name.");
        result = await api("scenarios", {
          name,
          between: [],
          edges: draftEdges(),
        });
        setPickingStreets(false);
      } else if (
        meta?.dataset === "v2" &&
        $("closure-picker").value === "route"
      ) {
        if (!route || route.mode === "transit" || route.nodes.length < 2)
          throw new Error("Find a car route first.");
        if (route.nodes.length > 201)
          throw new Error(
            "Choose a shorter route: closures support up to 200 directed segments.",
          );
        const edges = route.nodes.slice(0, -1).map((src, i) => ({
          src,
          dst: route.nodes[i + 1],
          edge_type: "street",
        }));
        result = await api("scenarios", {
          name: `Route to ${endpointName("to")}`,
          between: [],
          edges,
        });
      } else result = await api("close", { from: "city" });
      desk = result.desk;
      $("compare-result").hidden = true;
      await refreshMeta();
      await loadClosure();
      await routePair();
    }),
  );
  $("desk-select").addEventListener("change", () =>
    task("Opening scenario…", async () => {
      desk = $("desk-select").value;
      $("compare-result").hidden = true;
      renderMeta(meta);
      await loadClosure();
      await routePair();
    }),
  );
  $("btn-compare").addEventListener("click", () =>
    task("Comparing the two cities…", async () => {
      const result = await api("compare", { a: "city", b: desk });
      const text = `${result.graph_entities} graph changes · ${result.removed} removed · ${result.modified} modified · ${result.added} added`;
      $("compare-result").textContent = text;
      $("compare-result").hidden = false;
      setStatus("Comparison complete. The official city is unchanged.");
    }),
  );
  $("btn-archive").addEventListener("click", () =>
    task("Archiving scenario…", async () => {
      await api("archive", { desk });
      desk = null;
      deskRoute = null;
      closedStreets = [];
      $("compare-result").hidden = true;
      await refreshMeta();
      await loadClosure();
      await routePair(false);
    }),
  );
  $("btn-audit").addEventListener("click", () =>
    task("Checking city integrity…", async () => {
      try {
        const data = await api("audit", {});
        const result = $("audit-result");
        result.hidden = false;
        result.classList.toggle("error", !data.ok);
        result.textContent = data.ok
          ? `Verified. ${data.city_nodes.toLocaleString()} intersections, ${data.city_edges.toLocaleString()} connections, and ${data.desks.length} scenario histories checked.`
          : "Integrity check failed. See engine notes for details.";
        await refreshMeta();
        setStatus(
          data.ok ? "All integrity checks passed." : "Integrity check failed.",
          { error: !data.ok },
        );
      } catch (error) {
        $("audit-result").hidden = false;
        $("audit-result").classList.add("error");
        $("audit-result").textContent = error.message;
        throw error;
      }
    }),
  );

  function setEndpoint(which, value, label) {
    const select = $(`${which}-poi`);
    if (![...select.options].some((o) => o.value === value))
      select.add(new Option(label, value));
    select.value = value;
  }
  function mapClick(point) {
    if (!ready || busy) return;
    if (pickingStreets) {
      let nearest = null,
        distance = 14;
      for (const edge of streets) {
        const a = screen(city.nodes[edge.s]),
          b = screen(city.nodes[edge.d]);
        const dx = b.x - a.x,
          dy = b.y - a.y,
          length = dx * dx + dy * dy;
        const t = length
          ? Math.max(
              0,
              Math.min(
                1,
                ((point.x - a.x) * dx + (point.y - a.y) * dy) / length,
              ),
            )
          : 0;
        const d = Math.hypot(point.x - a.x - t * dx, point.y - a.y - t * dy);
        if (d < distance) {
          nearest = edge;
          distance = d;
        }
      }
      if (nearest) toggleClosureStreet(nearest);
      else
        setStatus("Click closer to a street, or zoom in to choose a section.");
      return;
    }
    if (meta?.dataset === "v2") {
      const hit = drawnPois.find((p) => {
        const q = screen(p);
        return Math.hypot(q.x - point.x, q.y - point.y) < 12;
      });
      if (hit) {
        showPlace(hit);
        return;
      }
    }
    if (selectedPlace) {
      dismissPlace(false);
      return;
    }
    if (activeMode !== "route") return;
    const target = world(point);
    let best = null,
      distance = 80 * 80;
    for (const node of city.nodes) {
      const d = (node.x - target.x) ** 2 + (node.y - target.y) ** 2;
      if (d <= distance && (!best || d < distance)) {
        distance = d;
        best = node;
      }
    }
    if (!best) {
      setStatus("Choose a point closer to a street intersection.");
      return;
    }
    if (!fromNode || toNode) {
      setEndpoint("from", best.id, "Selected intersection");
      setEndpoint("to", "", "Choose on map…");
      fromNode = best;
      toNode = null;
      clearRoutes();
      setStatus("Starting point set. Choose a destination on the map.", {
        persistent: true,
      });
    } else {
      setEndpoint("to", best.id, "Selected destination");
      toNode = best;
      task("Finding your route…", () => routePair(false));
    }
  }
  const pointers = new Map();
  let gesture = null,
    moved = false,
    multiTouch = false;
  canvas.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    cancelAnimationFrame(animation);
    canvas.setPointerCapture(e.pointerId);
    pointers.set(e.pointerId, eventPoint(e));
    if (pointers.size === 1) {
      gesture = eventPoint(e);
      moved = false;
      multiTouch = false;
    } else {
      multiTouch = true;
      moved = true;
    }
  });
  canvas.addEventListener("pointermove", (e) => {
    if (!pointers.has(e.pointerId)) return;
    const before = [...pointers.values()];
    const prev = pointers.get(e.pointerId),
      next = eventPoint(e);
    pointers.set(e.pointerId, next);
    if (pointers.size === 2) {
      const after = [...pointers.values()];
      const oldD = Math.hypot(
        before[0].x - before[1].x,
        before[0].y - before[1].y,
      );
      const newD = Math.hypot(after[0].x - after[1].x, after[0].y - after[1].y);
      if (oldD > 0)
        zoomAt(newD / oldD, {
          x: (after[0].x + after[1].x) / 2,
          y: (after[0].y + after[1].y) / 2,
        });
    } else if (pointers.size === 1) {
      if (gesture && Math.hypot(next.x - gesture.x, next.y - gesture.y) > 4)
        moved = true;
      if (moved) {
        camera.x -= (next.x - prev.x) / camera.scale;
        camera.y += (next.y - prev.y) / camera.scale;
        requestDraw();
      }
    }
  });
  function endPointer(e, cancelled = false) {
    if (!pointers.has(e.pointerId)) return;
    const point = eventPoint(e);
    pointers.delete(e.pointerId);
    if (canvas.hasPointerCapture(e.pointerId))
      canvas.releasePointerCapture(e.pointerId);
    if (!pointers.size && !moved && !multiTouch && !cancelled) mapClick(point);
  }
  canvas.addEventListener("pointerup", (e) => endPointer(e));
  canvas.addEventListener("pointercancel", (e) => endPointer(e, true));
  canvas.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      zoomAt(
        Math.exp(-Math.max(-100, Math.min(100, e.deltaY)) * 0.002),
        eventPoint(e),
      );
    },
    { passive: false },
  );
  canvas.addEventListener("keydown", (e) => {
    const pan = 90 / camera.scale;
    if (
      [
        "ArrowLeft",
        "ArrowRight",
        "ArrowUp",
        "ArrowDown",
        "+",
        "-",
        "=",
        "0",
      ].includes(e.key)
    )
      e.preventDefault();
    if (e.key === "ArrowLeft") camera.x -= pan;
    if (e.key === "ArrowRight") camera.x += pan;
    if (e.key === "ArrowUp") camera.y += pan;
    if (e.key === "ArrowDown") camera.y -= pan;
    if (e.key === "+" || e.key === "=") zoomAt(1.3);
    if (e.key === "-") zoomAt(1 / 1.3);
    if (e.key === "0") fitRoute();
    requestDraw();
  });
  $("btn-zoom-in").addEventListener("click", () => zoomAt(1.35));
  $("btn-zoom-out").addEventListener("click", () => zoomAt(1 / 1.35));
  $("btn-fit").addEventListener("click", () => fitRoute());
  $("nav-overview").addEventListener("click", () => {
    dismissPlace(false);
    if (city) fitPoints(city.nodes, true);
  });
  function updateActivity() {
    document.body.dataset.mode = activeMode;
    document.body.classList.toggle("place-open", Boolean(selectedPlace));
    $("panel-content").hidden = panelCollapsed;
    $("panel-toggle").setAttribute("aria-expanded", String(!panelCollapsed));
    $("panel-toggle").setAttribute(
      "aria-label",
      panelCollapsed ? "Expand panel" : "Collapse panel",
    );
    $("panel-toggle")
      .querySelector("use")
      .setAttribute("href", panelCollapsed ? "#i-plus" : "#i-minus");
    for (const button of document.querySelectorAll(
      ".activity-tabs [data-mode]",
    )) {
      const active = button.dataset.mode === activeMode;
      button.setAttribute("aria-selected", String(active));
      button.tabIndex = active ? 0 : -1;
      $(`pane-${button.dataset.mode}`).hidden =
        !active || Boolean(selectedPlace);
    }
    $("place-card").hidden = !selectedPlace;
    const transit = route?.mode === "transit";
    $("route-legend-label").textContent = transit ? "Walking" : "City route";
    $("route-legend-line").classList.toggle("walking", transit);
    $("transit-legend").hidden = !transit;
    $("desk-legend").hidden = transit || !desk;
    document.querySelector(".map-legend").hidden =
      activeMode === "explore" || !route || Boolean(selectedPlace);
    requestDraw();
  }
  function setMode(mode, focus = false) {
    dismissPlace(false);
    if (mode !== "closures" && pickingStreets) setPickingStreets(false);
    activeMode = mode;
    panelCollapsed = false;
    updateActivity();
    $("panel-content").scrollTop = 0;
    if (mode === "route" && route) fitRoute();
    if (focus) $(`nav-${mode}`).focus();
  }
  document.querySelectorAll(".activity-tabs [data-mode]").forEach((button) => {
    button.onclick = () => setMode(button.dataset.mode);
    button.onkeydown = (e) => {
      const tabs = [
        ...document.querySelectorAll(".activity-tabs [data-mode]"),
      ].filter((b) => !b.hidden);
      if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) return;
      e.preventDefault();
      const i = tabs.indexOf(button);
      const next =
        e.key === "Home"
          ? 0
          : e.key === "End"
            ? tabs.length - 1
            : (i + (e.key === "ArrowRight" ? 1 : -1) + tabs.length) %
              tabs.length;
      setMode(tabs[next].dataset.mode, true);
    };
  });
  $("panel-toggle").onclick = () => {
    panelCollapsed = !panelCollapsed;
    updateActivity();
  };
  function toggleLayers(open) {
    $("layers-popover").hidden = !open;
    $("nav-layers").setAttribute("aria-expanded", String(open));
    $("nav-layers").classList.toggle("active", open);
  }
  $("nav-layers").addEventListener("click", () =>
    toggleLayers($("layers-popover").hidden),
  );
  document.addEventListener("pointerdown", (e) => {
    if (!e.target.closest("#layers-popover, #nav-layers")) toggleLayers(false);
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && pickingStreets) {
      setPickingStreets(false);
      $("btn-pick-streets").focus();
    }
    if (e.key === "Escape" && !$("layers-popover").hidden) {
      toggleLayers(false);
      $("nav-layers").focus();
    } else if (
      e.key === "Escape" &&
      selectedPlace &&
      !$("system-dialog").open
    ) {
      dismissPlace();
    }
  });
  for (const id of ["show-pois", "show-streets", "show-subway", "subway-line"])
    $(id).addEventListener("change", requestDraw);
  for (const id of ["nav-system"])
    $(id).addEventListener("click", () => $("system-dialog").showModal());
  $("btn-dialog-close").addEventListener("click", () =>
    $("system-dialog").close(),
  );
  $("system-dialog").addEventListener("click", (e) => {
    if (e.target === $("system-dialog")) {
      const r = e.target.getBoundingClientRect();
      if (
        e.clientX < r.left ||
        e.clientX > r.right ||
        e.clientY < r.top ||
        e.clientY > r.bottom
      )
        e.target.close();
    }
  });
  function applyTheme() {
    document.body.classList.toggle("dark", dark);
    const label = `Switch to ${dark ? "light" : "dark"} mode`;
    $("btn-theme").setAttribute("aria-label", label);
    $("btn-theme").title = label;
    $("btn-theme")
      .querySelector("use")
      .setAttribute("href", dark ? "#i-sun" : "#i-moon");
    document.querySelector('meta[name="theme-color"]').content = dark
      ? "#22262d"
      : "#f7f8fa";
    requestDraw();
  }
  $("btn-theme").addEventListener("click", () => {
    dark = !dark;
    applyTheme();
    try {
      localStorage.setItem("island-theme", dark ? "dark" : "light");
    } catch {
      /* Keep the theme for this session. */
    }
  });
  new ResizeObserver(resize).observe(canvas);
  document.fonts.ready.then(requestDraw);
  applyTheme();
  resize();
  syncControls();
  let selectedPlace = null,
    placeCategory = "",
    placeCursor = null,
    placesRequest = 0,
    detailRequest = 0;
  const categorySymbols = {
    landmark: "◇",
    park: "♧",
    transit: "↔",
    address: "⌂",
  };
  function placeRow(place, caption) {
    rememberPlace(place);
    const button = document.createElement("button");
    button.type = "button";
    button.className = "place-result";
    const icon = document.createElement("span");
    icon.className = "place-symbol";
    icon.textContent = categorySymbols[place.category];
    const body = document.createElement("span"),
      name = document.createElement("strong"),
      sub = document.createElement("small");
    name.textContent = place.name;
    sub.textContent =
      caption ||
      `${place.subway ? "Subway station" : place.category}${place.node ? "" : " · No street access"}`;
    body.append(name, sub);
    button.append(icon, body);
    button.addEventListener("click", () => showPlace(place));
    return button;
  }
  async function browsePlaces(append = false) {
    const request = ++placesRequest;
    addressImpactCursor = null;
    browseAbort?.abort();
    browseAbort = new AbortController();
    if (!$("place-search").value.trim() && !placeCategory && !append) {
      const featured = [
        "p:way:22727025",
        "p:way:34633854",
        "p:way:265947358",
        "p:relation:7141751",
        "p:way:427818536",
      ];
      $("place-results").replaceChildren(
        ...featured
          .map((id) => pois.find((p) => p.id === id))
          .filter(Boolean)
          .map((p) => placeRow(p)),
      );
      $("places-status").textContent = "Start exploring";
      $("places-more").hidden = true;
      placeCursor = null;
      return;
    }
    const query = new URLSearchParams({
      q: $("place-search").value,
      category: placeCategory,
      limit: "5",
      branch: desk || "city",
    });
    if (append && placeCursor) query.set("cursor", placeCursor);
    $("places-status").textContent = "Finding places…";
    try {
      const data = await api(
        `${$("place-search").value.trim() ? "search" : "places"}?${query}`,
        undefined,
        browseAbort.signal,
      );
      if (request !== placesRequest) return;
      if (!append) $("place-results").replaceChildren();
      $("place-results").append(
        ...data.places.map((p) =>
          placeRow({ ...p, _version: data.version, _branch: data.branch }),
        ),
      );
      placeCursor = data.cursor;
      $("places-more").hidden = !placeCursor;
      $("places-status").textContent =
        data.hint ||
        (data.truncated
          ? "More matches · refine your search"
          : `${data.total.toLocaleString()} ${data.places.some((p) => p.category === "address") ? "results" : "places"}`);
    } catch (e) {
      if (request === placesRequest) $("places-status").textContent = e.message;
    }
  }
  function dismissPlace(restoreFocus = true) {
    if (!selectedPlace) return;
    selectedPlace = null;
    placeStack.length = 0;
    detailRequest++;
    updateActivity();
    if (restoreFocus) {
      if (placeOpener?.isConnected && placeOpener.getClientRects().length)
        placeOpener.focus({ preventScroll: true });
      else canvas.focus({ preventScroll: true });
    }
  }
  async function showPlace(place, back = false) {
    if (!back && selectedPlace?.id === place.id) {
      focusPlace(place);
      return;
    }
    if (!back) {
      if (!selectedPlace) placeOpener = document.activeElement;
      placeStack.push({
        place: selectedPlace,
        camera: { ...camera },
        scroll: $("panel-content").scrollTop,
      });
      if (placeStack.length > 20) placeStack.splice(1, 1);
    }
    if (pickingStreets) setPickingStreets(false);
    rememberPlace(place);
    selectedPlace = place;
    panelCollapsed = false;
    $("place-more").open = false;
    updateActivity();
    $("panel-content").scrollTop = 0;
    $("place-back").setAttribute(
      "aria-label",
      placeStack.at(-1)?.place ? "Back to previous place" : "Back to results",
    );
    const request = ++detailRequest;
    $("place-title").textContent =
      subway?.byId.get(place.id)?.name || place.name;
    $("place-kind").textContent = place.subway
      ? "SUBWAY STATION"
      : place.category;
    $("place-source").href = place.source_url;
    $("place-source").textContent = place.subway
      ? "View MTA station data ↗"
      : place.category === "address"
        ? "NYC address data ↗"
        : "View on OpenStreetMap ↗";
    $("address-tools").hidden = place.category !== "address";
    $("address-stations").disabled = !place.node;
    $("address-discover").disabled = !place.node;
    $("address-neighbors").replaceChildren();
    $("address-quality").textContent = "";
    $("subway-station").hidden = !place.subway;
    $("subway-neighbors").replaceChildren();
    $("subway-explore-status").textContent = "";
    $("subway-explore").disabled = false;
    if (place.subway) {
      $("subway-services").replaceChildren(
        ...place.subway.routes.map((id) => {
          const badge = document.createElement("span");
          badge.className = "subway-service";
          badge.textContent = id;
          badge.style.borderColor = subwayColor(id);
          return badge;
        }),
      );
      const siblings = pois.filter(
        (p) =>
          p.subway?.complex_id === place.subway.complex_id && p.id !== place.id,
      );
      $("subway-complex").textContent =
        `${place.subway.lines.join(" / ")}. ` +
        (siblings.length
          ? `Also in this complex: ${siblings.map((p) => p.name).join("; ")}.`
          : "Single-station complex.");
    }
    $("place-attachment").textContent = place.node
      ? `Street connection approximately ${place.approach_m} m away. Distances use this road anchor, not the entrance.`
      : "No connection to the street network. You can still explore this place on the map.";
    $("place-route").disabled =
      busy || (travelMode !== "transit" && !place.node);
    $("place-start").disabled =
      busy || (travelMode !== "transit" && !place.node);
    $("place-connections").textContent = "Loading connections…";
    $("relationship-view").replaceChildren();
    focusPlace(place);
    $("place-title").setAttribute("tabindex", "-1");
    $("place-title").focus({ preventScroll: true });
    try {
      const data = await api(
        `${place.category === "address" ? "addresses" : "places"}/${encodeURIComponent(place.id)}?branch=${encodeURIComponent(desk || "city")}${place.category === "address" && place._branch === (desk || "city") ? `&version=${place._version}` : ""}`,
      );
      if (request !== detailRequest) return;
      if (data.address) {
        const a = data.address;
        $("address-quality").textContent =
          `${a.zip ? `Manhattan ${a.zip} · ` : ""}${a.source.address_status === "4" ? "Built" : "Building status unverified"}${a.source.house_number_range ? " · Recorded address range" : ""}. ${a.node ? "Approximate street routing." : "Street routing unavailable here."}`;
      }
      $("place-connections").replaceChildren(
        ...data.connections.map((c) => {
          const row = document.createElement("div");
          row.className = "connection-row";
          row.textContent =
            place.category === "address"
              ? c.relation === "near_road"
                ? `Street connection · approximately ${place.approach_m} m`
                : `${c.relation.replaceAll("_", " ")} · ${c.properties?.name || c.id}`
              : c.relation.endsWith("in_category")
                ? `Category · ${place.category}`
                : `Nearby intersection · ${place.approach_m} m approach`;
          return row;
        }),
      );
    } catch (e) {
      if (request === detailRequest)
        $("place-connections").textContent = e.message;
    }
  }
  $("btn-graph-metrics").onclick = async () => {
    try {
      const m = await api("graph-metrics");
      $("graph-metrics-output").textContent =
        `Completed jobs: ${m.completed_jobs}\nTotal service time: ${m.total_service_ms.toFixed(1)} ms\nActive jobs: ${m.active_jobs} / 2\nAdmitted jobs: ${m.admitted_jobs} / 10`;
    } catch (e) {
      $("graph-metrics-output").textContent = e.message;
    }
  };
  $("subway-explore").onclick = async () => {
    if (!selectedPlace?.subway) return;
    const request = detailRequest;
    $("subway-explore").disabled = true;
    $("subway-explore-status").textContent = "Exploring subway connections…";
    try {
      const result = await api("subway/explore", {
        branch: desk || "city",
        seed: selectedPlace.id,
        depth: 2,
      });
      if (request !== detailRequest) return;
      $("subway-explore-status").textContent =
        `${result.stations.length - 1} stations within two stops or transfers. Express services may skip stops.`;
      $("subway-neighbors").replaceChildren(
        ...result.stations
          .filter((s) => s.id !== selectedPlace.id)
          .map((s) =>
            placeRow(
              pois.find((p) => p.id === s.id),
              s.routes.join(" · "),
            ),
          ),
      );
    } catch (e) {
      if (request === detailRequest)
        $("subway-explore-status").textContent = e.message;
    } finally {
      if (request === detailRequest) $("subway-explore").disabled = false;
    }
  };
  $("place-dismiss").onclick = () => dismissPlace();
  $("place-back").onclick = () => {
    const previous = placeStack.pop();
    if (previous?.place) showPlace(previous.place, true);
    else dismissPlace();
    if (previous) {
      moveCamera(previous.camera);
      $("panel-content").scrollTop = previous.scroll;
    }
  };
  for (const [id, which] of [
    ["place-start", "from"],
    ["place-route", "to"],
  ])
    $(id).onclick = () => {
      if (
        busy ||
        !selectedPlace ||
        (travelMode !== "transit" && !selectedPlace.node)
      )
        return;
      setEndpoint(which, selectedPlace.id, selectedPlace.name);
      readEndpoints();
      setMode("route");
      task("Finding your route…", routePair);
    };
  $("place-search").addEventListener("input", () => {
    placesRequest++;
    browseAbort?.abort();
    clearTimeout(browseTimer);
    browseTimer = setTimeout(() => browsePlaces(), 150);
  });
  $("place-search").addEventListener("keydown", (e) => {
    if (["ArrowDown", "Enter"].includes(e.key)) {
      e.preventDefault();
      $("place-results").querySelector("button")?.focus();
    }
  });
  document.querySelectorAll("[data-category]").forEach(
    (button) =>
      (button.onclick = () => {
        placeCategory = button.dataset.category;
        document
          .querySelectorAll("[data-category]")
          .forEach((b) => b.setAttribute("aria-pressed", String(b === button)));
        browsePlaces();
        requestDraw();
      }),
  );
  $("places-more").onclick = () =>
    addressImpactCursor ? showAddressImpact(true) : browsePlaces(true);
  $("btn-discover").onclick = async () => {
    setMode("explore");
    const request = ++placesRequest;
    $("places-status").textContent = "Following the street network…";
    $("places-more").hidden = true;
    try {
      const data = await api("discover", {
        origin: $("from-poi").value,
        branch: desk || "city",
        category: placeCategory || null,
        max_m: Number($("discovery-radius").value),
        version:
          desk && $("scenario-version").value
            ? Number($("scenario-version").value)
            : null,
      });
      if (request !== placesRequest) return;
      $("place-results").replaceChildren(
        ...data.results.map((r) =>
          placeRow(r.place, `${formatDistance(r.distance_m)} by road`),
        ),
      );
      $("places-status").textContent =
        `${data.results.length} reachable · ${data.unreachable} unconnected or unreachable · ${desk ? "Scenario" : "Official city"}`;
    } catch (e) {
      if (request === placesRequest) $("places-status").textContent = e.message;
    }
  };
  $("btn-impact").onclick = async () => {
    setMode("explore");
    const request = ++placesRequest;
    $("places-more").hidden = true;
    if (!desk) {
      $("places-status").textContent =
        "Create a closure scenario to compare destination access.";
      return;
    }
    $("places-status").textContent = "Comparing network distances…";
    try {
      const data = await api(`scenarios/${encodeURIComponent(desk)}/impact`, {
        origin: $("from-poi").value,
      });
      if (request !== placesRequest) return;
      const changed = data.results.filter(
        (r) => !["unchanged", "already_unreachable"].includes(r.status),
      );
      $("place-results").replaceChildren(
        ...changed.map((r) =>
          placeRow(
            r.place,
            r.status === "farther"
              ? `+${formatDistance(r.after_m - r.before_m)} detour`
              : r.status.replaceAll("_", " "),
          ),
        ),
      );
      const unreachable = data.results.filter(
        (r) => r.status === "already_unreachable",
      ).length;
      $("places-status").textContent =
        `${changed.length} affected destinations · ${unreachable} already unreachable`;
    } catch (e) {
      if (request === placesRequest) $("places-status").textContent = e.message;
    }
  };
  $("place-explore").onclick = async () => {
    if (!selectedPlace) return;
    const request = detailRequest;
    $("relationship-view").textContent = "Exploring relationships…";
    try {
      const data = await api(
        selectedPlace.category === "address" ? "address-explore" : "explore",
        {
          seed: selectedPlace.id,
          branch: desk || "city",
          depth: 2,
          limit: 16,
        },
      );
      if (request !== detailRequest) return;
      const ns = "http://www.w3.org/2000/svg",
        svg = document.createElementNS(ns, "svg");
      svg.setAttribute("viewBox", "0 0 460 240");
      svg.classList.add("relationship-svg");
      svg.setAttribute("role", "img");
      svg.setAttribute("aria-label", `Connections for ${selectedPlace.name}`);
      const coords = new Map(
        data.nodes.map((n, i) => [
          n.id,
          i === 0
            ? [230, 120]
            : [
                230 +
                  170 *
                    Math.cos(((i - 1) * 2 * Math.PI) / (data.nodes.length - 1)),
                120 +
                  88 *
                    Math.sin(((i - 1) * 2 * Math.PI) / (data.nodes.length - 1)),
              ],
        ]),
      );
      for (const e of data.edges) {
        const a = coords.get(e.source),
          b = coords.get(e.target),
          line = document.createElementNS(ns, "line");
        for (const [k, v] of Object.entries({
          x1: a[0],
          y1: a[1],
          x2: b[0],
          y2: b[1],
          stroke: "#8b9ba5",
          "stroke-width": 1,
        }))
          line.setAttribute(k, v);
        svg.append(line);
      }
      for (const n of data.nodes) {
        const [x, y] = coords.get(n.id),
          circle = document.createElementNS(ns, "circle"),
          label = document.createElementNS(ns, "text");
        circle.setAttribute("cx", x);
        circle.setAttribute("cy", y);
        circle.setAttribute("r", n.id === selectedPlace.id ? 7 : 4);
        circle.setAttribute(
          "fill",
          n.id === selectedPlace.id ? "#527e9a" : "#81968b",
        );
        label.setAttribute("x", x);
        label.setAttribute("y", y + 17);
        label.setAttribute("text-anchor", "middle");
        label.textContent =
          n.name.length > 23 ? n.name.slice(0, 21) + "…" : n.name;
        svg.append(circle, label);
      }
      const note = document.createElement("p");
      note.className = "places-status";
      note.textContent = `${data.nodes.length} connections shown${data.truncated ? " · Limited to two hops and 16 nodes" : ""}`;
      $("relationship-view").replaceChildren(svg, note);
      if (selectedPlace.category === "address") {
        const related = data.nodes.filter(
          (n) => n.id.startsWith("a:nyc:") && n.id !== selectedPlace.id,
        );
        for (const n of related) {
          const b = document.createElement("button");
          b.className = "text-button";
          b.textContent = n.name;
          b.onclick = async () => {
            const d = await api(
              `addresses/${encodeURIComponent(n.id)}?branch=${encodeURIComponent(desk || "city")}`,
            );
            showPlace(d.address);
          };
          $("relationship-view").append(b);
        }
      }
    } catch (e) {
      if (request === detailRequest)
        $("relationship-view").textContent = e.message;
    }
  };
  $("address-discover").onclick = async () => {
    const request = detailRequest;
    $("address-neighbors").textContent = "Exploring nearby streets…";
    try {
      const d = await api("address-discover", {
        branch: desk || "city",
        origin: selectedPlace.id,
        max_m: 1000,
      });
      if (request !== detailRequest) return;
      const note = document.createElement("p");
      note.className = "places-status";
      note.textContent = `${d.total.toLocaleString()} addresses within 1 km by street${d.truncated ? " · showing nearest 100" : ""}`;
      $("address-neighbors").replaceChildren(
        note,
        ...d.results.map((r) =>
          placeRow(
            r.place,
            `${formatDistance(r.network_distance_m)} by street`,
          ),
        ),
      );
    } catch (e) {
      if (request === detailRequest)
        $("address-neighbors").textContent = e.message;
    }
  };
  $("address-stations").onclick = async () => {
    const request = detailRequest;
    $("address-neighbors").textContent = "Following the street network…";
    try {
      const d = await api(
        `addresses/${encodeURIComponent(selectedPlace.id)}/nearest-stations`,
        { branch: desk || "city" },
      );
      if (request !== detailRequest) return;
      $("address-neighbors").replaceChildren(
        ...d.results.map((r) =>
          placeRow(
            r.place,
            `${formatDistance(r.network_distance_m)} by street · approximate anchors`,
          ),
        ),
      );
      if (!d.results.length)
        $("address-neighbors").textContent =
          "No stations reachable on this street network.";
    } catch (e) {
      if (request === detailRequest)
        $("address-neighbors").textContent = e.message;
    }
  };
  async function showAddressImpact(append = false) {
    setMode("explore");
    const request = ++placesRequest;
    if (!desk) {
      $("places-status").textContent = "Create a closure scenario first.";
      return;
    }
    if (!append) {
      addressImpactOrigin = $("from-poi").value;
      addressImpactCursor = null;
    }
    $("places-status").textContent = "Comparing address access…";
    try {
      const d = await api(
        `scenarios/${encodeURIComponent(desk)}/address-impact`,
        {
          origin: addressImpactOrigin,
          status: "affected",
          limit: 20,
          cursor: append ? addressImpactCursor : null,
        },
      );
      if (request !== placesRequest) return;
      if (!append) $("place-results").replaceChildren();
      $("place-results").append(
        ...d.results.map((r) =>
          placeRow(
            r.place,
            r.status === "farther"
              ? `+${formatDistance(r.after_m - r.before_m)} detour`
              : r.status.replaceAll("_", " "),
          ),
        ),
      );
      addressImpactCursor = d.cursor;
      $("places-more").hidden = !d.cursor;
      $("places-status").textContent =
        `${d.total.toLocaleString()} affected addresses · ${(d.counts.unconnected || 0).toLocaleString()} without street connections`;
    } catch (e) {
      if (request === placesRequest) $("places-status").textContent = e.message;
    }
  }
  $("btn-address-impact").onclick = () => showAddressImpact();
  let scenarioHistory = null;
  async function loadScenarioHistory() {
    $("scenario-history-controls").hidden = !desk;
    if (!desk) {
      scenarioHistory = null;
      return;
    }
    scenarioHistory = await api(
      `scenarios/${encodeURIComponent(desk)}/history`,
    );
    const last = scenarioHistory.operations.at(-1);
    $("scenario-version").replaceChildren(
      new Option("Before closure", scenarioHistory.parent_version),
      ...scenarioHistory.operations.map(
        (op, i) =>
          new Option(
            `${i + 1}. ${op.closed ? "Closed" : "Reopened"} · v${op.version}`,
            op.version,
          ),
      ),
    );
    if (scenarioHistory.current_version !== last.version)
      $("scenario-version").add(
        new Option(
          "Current · expanded catalog",
          scenarioHistory.current_version,
        ),
      );
    $("scenario-version").value = scenarioHistory.current_version;
    $("btn-reopen").textContent = last.closed
      ? "Reopen corridor"
      : "Close corridor again";
    $("closure-tag").textContent = last.closed ? "CLOSED" : "REOPENED";
    document.querySelector(".closure-street strong").textContent =
      scenarioHistory.name;
    document.querySelector(".closure-street div > span").textContent =
      `${scenarioHistory.closure.edges.length} directed street segments`;
  }
  $("scenario-version").onchange = () => {
    placesRequest++;
    $("places-status").textContent =
      "Select Reachable from start to explore this version.";
  };
  $("btn-reopen").onclick = () =>
    task("Updating the corridor…", async () => {
      if (!desk || !scenarioHistory) return;
      const last = scenarioHistory.operations.at(-1);
      await api(`scenarios/${encodeURIComponent(desk)}/operations`, {
        id: crypto.randomUUID(),
        closed: !last.closed,
        expected_version: scenarioHistory.current_version,
      });
      await refreshMeta();
      await loadClosure();
      await routePair();
    });
  function setupPlaceCombobox(which) {
    const select = $(`${which}-poi`),
      label = select.parentElement,
      input = document.createElement("input"),
      list = document.createElement("div");
    select.hidden = true;
    input.className = "endpoint-search";
    input.id = `${which}-search`;
    input.setAttribute("role", "combobox");
    input.setAttribute("aria-autocomplete", "list");
    input.setAttribute("aria-expanded", "false");
    input.setAttribute(
      "aria-label",
      which === "from" ? "Starting point" : "Destination",
    );
    input.setAttribute("autocomplete", "off");
    list.id = `${which}-suggestions`;
    list.className = "endpoint-suggestions";
    list.setAttribute("role", "listbox");
    list.hidden = true;
    input.setAttribute("aria-controls", list.id);
    label.append(input, list);
    let active = -1,
      choices = [],
      request = 0,
      timer,
      abort;
    const close = () => {
      list.hidden = true;
      input.setAttribute("aria-expanded", "false");
      input.removeAttribute("aria-activedescendant");
    };
    const choose = (p) => {
      if (busy) return;
      request++;
      abort?.abort();
      clearTimeout(timer);
      rememberPlace(p);
      setEndpoint(which, p.id, p.name);
      readEndpoints();
      close();
      input.focus();
    };
    const paint = (rows) => {
      active = -1;
      choices = rows
        .filter((p) => travelMode === "transit" || p.node)
        .slice(0, 6);
      list.replaceChildren(
        ...choices.map((p, i) => {
          const b = document.createElement("button");
          b.type = "button";
          b.id = `${which}-choice-${i}`;
          b.setAttribute("role", "option");
          b.setAttribute("aria-selected", "false");
          b.tabIndex = -1;
          b.textContent = p.name;
          b.onmousedown = (e) => e.preventDefault();
          b.onclick = () => choose(p);
          return b;
        }),
      );
      list.hidden = !choices.length;
      input.setAttribute("aria-expanded", String(!!choices.length));
    };
    const render = () => {
      const n = ++request,
        text = input.value;
      abort?.abort();
      clearTimeout(timer);
      paint(
        pois.filter((p) =>
          text
            .toLowerCase()
            .trim()
            .split(/\s+/)
            .every((t) => p.name.toLowerCase().includes(t)),
        ),
      );
      timer = setTimeout(async () => {
        abort = new AbortController();
        try {
          const d = await api(
            `search?${new URLSearchParams({ q: text, branch: desk || "city", limit: "20" })}`,
            undefined,
            abort.signal,
          );
          if (n === request && document.activeElement === input)
            paint(d.places);
        } catch (e) {
          if (n === request && e.name !== "AbortError")
            input.setAttribute("aria-description", e.message);
        }
      }, 150);
    };
    input.oninput = render;
    input.onfocus = () => input.select();
    input.onblur = () => {
      request++;
      clearTimeout(timer);
      abort?.abort();
      close();
      input.value = endpointName(which);
    };
    input.onkeydown = (e) => {
      if (e.key === "Escape") {
        close();
        return;
      }
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        if (list.hidden) render();
        active = Math.max(
          0,
          Math.min(
            choices.length - 1,
            active + (e.key === "ArrowDown" ? 1 : -1),
          ),
        );
        [...list.children].forEach((b, i) =>
          b.setAttribute("aria-selected", String(i === active)),
        );
        if (active >= 0)
          input.setAttribute(
            "aria-activedescendant",
            `${which}-choice-${active}`,
          );
      }
      if (e.key === "Enter" && !list.hidden) {
        e.preventDefault();
        if (choices[active < 0 ? 0 : active])
          choose(choices[active < 0 ? 0 : active]);
      }
    };
    input.value = endpointName(which);
  }

  async function init() {
    try {
      const [c, g, m] = await Promise.all([
        api("city"),
        api("gazetteer"),
        api("meta"),
      ]);
      city = c;
      pois = g;
      if (m.dataset === "v2") {
        const catalog = await api("places?limit=100");
        pois = [...catalog.places];
        let cursor = catalog.cursor;
        while (cursor) {
          const page = await api(
            `places?limit=100&version=${catalog.version}&cursor=${encodeURIComponent(cursor)}`,
          );
          pois.push(...page.places);
          cursor = page.cursor;
        }
        if (
          pois.length !== catalog.total ||
          new Set(pois.map((p) => p.id)).size !== catalog.total
        )
          throw new Error("The destination catalog is incomplete.");
        $("place-count").textContent = `${pois.length} PLACES`;
        $("place-browser").hidden = false;
        $("closure-picker-label").hidden = false;
        $("mission-panel").hidden = false;
        subway = await api("subway");
        subway.byId = new Map(subway.stations.map((s) => [s.id, s]));
        $("subway-controls").hidden = false;
        $("subway-summary").textContent =
          `${subway.stations.length} stations · ${new Set(subway.stations.map((s) => s.complex_id)).size} complexes · ${subway.edges.length} directed connections`;
        $("subway-line").append(
          ...subway.routes.map(
            (r) => new Option(`${r.name} · ${r.description}`, r.id),
          ),
        );
      }
      nodes = new Map(c.nodes.map((n) => [n.id, n]));
      makeStreetIndex();
      for (const which of ["from", "to"])
        $(`${which}-poi`).replaceChildren(
          ...pois
            .filter((p) => m.dataset !== "v2" || p.aliases?.length)
            .map((p) => new Option(p.name, p.id)),
        );
      $("from-poi").value =
        pois.find((p) => p.aliases?.includes("poi:port-authority"))?.id ||
        "poi:port-authority";
      $("to-poi").value =
        pois.find((p) => p.aliases?.includes("poi:grand-central"))?.id ||
        "poi:grand-central";
      if (m.dataset === "v2") {
        $("travel-modes").hidden = false;
        setupPlaceCombobox("from");
        setupPlaceCombobox("to");
      }
      readEndpoints();
      ready = true;
      if (m.dataset === "v2") restoreMission();
      renderMeta(m);
      if (m.dataset !== "v2") {
        $("nav-explore").hidden = true;
        $("nav-missions").hidden = true;
        activeMode = "route";
      }
      updateActivity();
      await task("Loading Manhattan…", async () => {
        await loadClosure();
        await routePair(false);
      });
      moveCamera(
        centeredCamera({ x: 3000, y: 5900 }, Math.min(0.25, width / 4800)),
        false,
      );
      document.body.dataset.ready = "true";
      if (m.dataset === "v2") browsePlaces();
    } catch (error) {
      document.body.dataset.ready = "error";
      setStatus(`${error.message} Reload the page to retry.`, { error: true });
    }
  }
  init();
})();
