const $ = (id) => document.getElementById(id);
const svg = (tag, attrs = {}, content) => {
  const node = document.createElementNS("http://www.w3.org/2000/svg", tag);
  for (const [key, value] of Object.entries(attrs))
    node.setAttribute(key, value);
  if (content !== undefined) node.textContent = content;
  return node;
};

export class BranchExplorer {
  constructor({ label, request, navigate, redraw }) {
    Object.assign(this, { label, request, navigate, redraw });
    this.nodes = new Map();
    this.cache = new Map();
    this.pending = null;
    this.failed = null;
    $("chart-retry").onclick = () => {
      this.failed = null;
      this.redraw();
    };
    $("chart-values").ontoggle = () => this.renderTable();
  }
  update(input) {
    this.input = input;
    this.renderLineage();
    this.renderChart();
  }
  renderLineage() {
    const { state, selected, locked } = this.input;
    const names = new Set(state.branches.map((b) => b.name));
    for (const [name, node] of this.nodes)
      if (!names.has(name)) {
        node.li.remove();
        this.nodes.delete(name);
      }
    for (const branch of state.branches) {
      let node = this.nodes.get(branch.name);
      if (!node) {
        const li = document.createElement("li"),
          row = document.createElement("div");
        row.className = "lineage-row";
        const choose = document.createElement("button"),
          title = document.createElement("strong"),
          meta = document.createElement("span");
        choose.className = "lineage-choice";
        choose.dataset.lineage = branch.name;
        title.textContent = this.label(branch.name);
        choose.append(title, meta);
        choose.onclick = () => this.navigate(branch.name);
        const origin = document.createElement("button");
        origin.className = "text-button lineage-origin";
        origin.dataset.origin = branch.name;
        // Resolve against current state so a reset cannot retain old coordinates.
        origin.onclick = () => {
          const parent = this.input.state.branches.find(
            (b) => b.name === branch.name,
          ).parent;
          this.navigate(parent.name, parent.checkpoint);
        };
        const children = document.createElement("ul");
        children.className = "lineage-list";
        row.append(choose, origin);
        li.append(row, children);
        node = { li, choose, meta, origin, children };
        this.nodes.set(branch.name, node);
      }
      const container = branch.parent
        ? this.nodes.get(branch.parent.name).children
        : $("lineage");
      if (node.li.parentNode !== container) container.append(node.li);
      node.choose.disabled = locked;
      node.origin.disabled = locked || !branch.parentAvailable;
      node.choose.setAttribute(
        "aria-pressed",
        String(branch.name === selected),
      );
      node.meta.textContent = `Gen ${branch.head.generation} · rev ${branch.head.revision}`;
      node.origin.hidden = !branch.parent;
      if (branch.parent) {
        const { name, generation, revision } = branch.parent;
        node.origin.textContent = `Fork: gen ${generation} · rev ${revision}${branch.parentAvailable ? " ↗" : " · expired"}`;
        node.origin.setAttribute(
          "aria-label",
          `Visit ${this.label(branch.name)} fork point in ${this.label(name)}, generation ${generation}, revision ${revision}`,
        );
        node.origin.title = branch.parentAvailable
          ? `From ${this.label(name)}. Open the exact source moment.`
          : "The fork point has rolled out of recent history. This world still lives on.";
      }
      node.children.hidden = !state.branches.some(
        (b) => b.parent?.name === branch.name,
      );
    }
    const path = [];
    let branch = state.branches.find((b) => b.name === selected);
    while (branch) {
      path.unshift(this.label(branch.name));
      branch = state.branches.find((b) => b.name === branch.parent?.name);
    }
    $("lineage-context").textContent = path.join(" → ");
  }
  renderChart() {
    const { state, selected, reference, epoch } = this.input;
    const left = state.branches.find((b) => b.name === selected);
    const right = state.branches.find((b) => b.name === reference);
    $("chart-pair").textContent = right
      ? `${this.label(selected)} / ${this.label(reference)}`
      : "No reference";
    const key = right ? `${epoch}|${left.head.id}|${right.head.id}` : null;
    this.chartKey = key;
    const result = this.cache.get(key);
    this.points = result?.points ?? [];
    $("chart-retry").hidden = this.failed?.key !== key || !key;
    $("chart-values").hidden = !this.points.length;
    // SVG elements do not implement HTMLElement.hidden; set the attribute.
    $("divergence-chart").toggleAttribute("hidden", !this.points.length);
    $("chart-status").textContent = !right
      ? "Control has no parent. Choose Control as the reference."
      : this.failed?.key === key
        ? `Could not load saved comparisons: ${this.failed.message}`
        : !result
          ? "Loading saved comparisons…"
          : !this.points.length
            ? "These colonies have no recorded generations in common."
            : `${this.points.length} matching generation${this.points.length === 1 ? "" : "s"} · ${this.points.at(-1).divergence.toLocaleString("en-US")} cells differ at gen ${this.points.at(-1).generation}`;
    if (result && this.drawnKey !== key) {
      this.drawnKey = key;
      this.drawChart();
      this.renderTable();
    }
    if (!key || result || this.pending || this.failed?.key === key) return;
    this.pending = key;
    this.request("comparison-history", {
      left: selected,
      right: reference,
      leftHead: left.head.id,
      rightHead: right.head.id,
    })
      .then((result) => {
        this.cache.set(key, result);
        if (this.cache.size > 12)
          this.cache.delete(this.cache.keys().next().value);
      })
      .catch((error) => {
        this.failed = { key, message: error.message };
      })
      .finally(() => {
        this.pending = null;
        this.redraw();
      });
  }
  drawChart() {
    const points = this.points;
    if (!points.length) {
      $("divergence-chart").replaceChildren();
      return;
    }
    const first = points[0].generation,
      last = points.at(-1).generation;
    const high = Math.max(1, ...points.map((p) => p.divergence));
    const x = (g) =>
      last === first ? 330 : 52 + ((g - first) / (last - first)) * 554;
    const y = (value) => 132 - (value / high) * 96;
    const nodes = [svg("text", { x: 12, y: 16 }, "Different cells")];
    for (const value of [0, high]) {
      nodes.push(
        svg("line", {
          x1: 52,
          x2: 606,
          y1: y(value),
          y2: y(value),
          class: "chart-gridline",
        }),
      );
      nodes.push(
        svg(
          "text",
          { x: 44, y: y(value) + 4, "text-anchor": "end" },
          String(value),
        ),
      );
    }
    nodes.push(
      svg("polyline", {
        points: points
          .map((p) => `${x(p.generation)},${y(p.divergence)}`)
          .join(" "),
        class: "chart-line",
      }),
    );
    for (const p of [points[0], ...(points.length > 1 ? [points.at(-1)] : [])])
      nodes.push(
        svg("circle", {
          cx: x(p.generation),
          cy: y(p.divergence),
          r: 3,
          class: "chart-dot",
        }),
      );
    for (const g of new Set([first, last]))
      nodes.push(
        svg("text", { x: x(g), y: 153, "text-anchor": "middle" }, String(g)),
      );
    nodes.push(
      svg("text", { x: 330, y: 175, "text-anchor": "middle" }, "Generation"),
    );
    $("divergence-chart").replaceChildren(...nodes);
    $("divergence-chart").setAttribute(
      "aria-label",
      `Saved differences between ${this.label(this.input.selected)} and ${this.label(this.input.reference)}, generations ${first} to ${last}. Latest difference ${points.at(-1).divergence} cells. Values available in the table.`,
    );
  }
  renderTable() {
    if (!$("chart-values").open) return;
    const fragment = document.createDocumentFragment();
    for (const point of this.points ?? []) {
      const row = document.createElement("tr");
      for (const value of [
        point.generation,
        point.leftRevision,
        point.rightRevision,
        point.divergence,
      ]) {
        const cell = document.createElement("td");
        cell.textContent = value;
        row.append(cell);
      }
      fragment.append(row);
    }
    $("chart-table-caption").textContent = $("chart-pair").textContent;
    $("chart-rows").replaceChildren(fragment);
  }
}
