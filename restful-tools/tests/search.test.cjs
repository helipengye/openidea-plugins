const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

class Element {
  constructor(tagName = "div") {
    this.tagName = tagName;
    this.children = [];
    this.text = "";
    this.value = "";
    this.dataset = {};
    this.attributes = {};
    this.listeners = {};
    this.classList = { add() {} };
  }
  get textContent() { return this.text + this.children.map((child) => child.textContent).join(""); }
  set textContent(value) { this.text = String(value); this.children = []; }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.text = ""; this.children = children; }
  setAttribute(name, value) { this.attributes[name] = value; }
  addEventListener(name, listener) { this.listeners[name] = listener; }
  querySelectorAll(selector) {
    return this.children.flatMap((child) => [
      ...((selector.startsWith(".") ? child.className === selector.slice(1) : child.tagName === selector) ? [child] : []),
      ...child.querySelectorAll(selector),
    ]);
  }
}

const endpoints = [
  { module: "app", method: "GET", url: "/api/goods/sales/detail", controller: "GoodsController",
    controller_label: "", name: "querySalesDetail", label: "商品销售明细", path: "/project/Api.java", line: 0 },
  { module: "app", method: "POST", url: "/api/orders", controller: "OtherController",
    controller_label: "", name: "createOrder", label: "新增订单", path: "/project/Other.java", line: 0 },
];

async function panel(rows = endpoints) {
  const elements = new Map();
  const element = (id) => {
    if (!elements.has(id)) elements.set(id, new Element());
    return elements.get(id);
  };
  const messages = [];
  const timers = new Map();
  let timerId = 0;
  let receive;
  const web = path.join(__dirname, "../web");
  const context = vm.createContext({
    navigator: { language: "en" },
    TextEncoder,
    clearTimeout(id) { timers.delete(id); },
    setTimeout(callback, delay) { timers.set(++timerId, { callback, delay }); return timerId; },
    fetch: async (url) => ({ ok: true, json: async () => JSON.parse(fs.readFileSync(path.join(web, url), "utf8")) }),
    document: {
      documentElement: {},
      getElementById: element,
      querySelectorAll: () => [],
      createElement: (tagName) => new Element(tagName),
      createElementNS: () => new Element(),
      createTextNode: (text) => { const node = new Element("#text"); node.textContent = text; return node; },
    },
    window: { openidea: {
      postMessage(text) { messages.push(JSON.parse(text)); },
      onMessage(callback) { receive = callback; },
    } },
  });
  await vm.runInContext(fs.readFileSync(path.join(web, "page.js"), "utf8"), context);
  const client = messages[0].client;
  const send = (message) => receive(JSON.stringify({ client, root: "/project", ...message }));
  send({ op: "ready", settings: { active: "local", environments: { local: {} } } });
  send({ op: "scan-begin", generation: 1 });
  send({ op: "scan-page", generation: 1, offset: 0, endpoints: rows });
  send({ op: "scan", continue: false, scan: {
    generation: 1, files: rows.length, total_files: rows.length, skipped: 0, done: true, cached: true, saved_at: 1,
  } });
  const flush = () => {
    for (const { callback, delay } of timers.values()) {
      assert.equal(delay, 120);
      callback();
    }
    timers.clear();
  };
  const input = (query) => {
    element("search").value = query;
    element("search").listeners.input();
  };
  const search = (query, expand = true) => {
    input(query);
    flush();
    const titles = [];
    function visit(node) {
      if (expand && node.className === "tree-node" && !node.open) {
        node.open = true;
        node.listeners.toggle();
      }
      if (node.className === "endpoint") titles.push(node.title);
      for (const child of node.children) visit(child);
    }
    visit(element("tree"));
    return titles;
  };
  return { search, input, flush, tree: element("tree") };
}

test("ordered character search keeps whitespace AND semantics for cached results", async () => {
  const { search } = await panel();
  const goods = ["/project/Api.java:1"];
  const orders = ["/project/Other.java:1"];
  for (const [query, expected] of [
    ["gdsdtl", goods],
    ["GET\t GDSDTL", goods],
    ["qsd", goods],
    ["商细", goods],
    ["sales", goods],
    ["goods detail", goods],
    ["post crord", orders],
    ["orders", orders],
    ["appget", []],
    ["GET missing", []],
    ["ldsg", []],
    ["gg", []],
    ["goods*detail", []],
    ["  ", [...goods, ...orders]],
  ]) assert.deepEqual(search(query), expected, query);
});

test("highlights ordered characters, merges adjacent and overlapping hits, and clears highlights", async () => {
  const { search, tree } = await panel();
  search("gdsdtl");
  const route = tree.querySelectorAll(".route")[0];
  assert.deepEqual(route.querySelectorAll("mark").map((node) => node.textContent), ["g", "ds", "d", "t", "l"]);
  assert.equal(route.textContent, "商品销售明细  /api/goods/sales/detailquerySalesDetail · 1");
  search("sales ales");
  assert.deepEqual(tree.querySelectorAll(".route")[0].querySelectorAll("mark").map((node) => node.textContent), ["sales", "Sales"]);
  search("GET qsd");
  assert.deepEqual(tree.querySelectorAll(".verb")[0].querySelectorAll("mark").map((node) => node.textContent), ["GET"]);
  assert.deepEqual(tree.querySelectorAll("small")[0].querySelectorAll("mark").map((node) => node.textContent), ["q", "S", "D"]);
  search("商细");
  assert.deepEqual(tree.querySelectorAll(".route")[0].querySelectorAll("mark").map((node) => node.textContent), ["商", "细"]);
  search("");
  assert.equal(tree.querySelectorAll("mark").length, 0);
});

test("highlighting preserves original Unicode and literal HTML text, and the selected row", async () => {
  const row = { ...endpoints[0], label: "İ😀<script>商品</script>" };
  const { search, tree } = await panel([row]);
  search("😀商");
  assert.deepEqual(tree.querySelectorAll(".route")[0].querySelectorAll("mark").map((node) => node.textContent), ["😀", "商"]);
  search("i😀");
  assert.deepEqual(tree.querySelectorAll(".route")[0].querySelectorAll("mark").map((node) => node.textContent), ["İ😀"]);
  assert.equal(tree.querySelectorAll("script").length, 0);
  assert.ok(tree.querySelectorAll(".route")[0].textContent.startsWith(row.label));
  const button = tree.querySelectorAll(".endpoint")[0];
  const highlightCount = tree.querySelectorAll("mark").length;
  button.listeners.click();
  assert.equal(button.attributes["aria-pressed"], "true");
  assert.equal(tree.querySelectorAll("mark").length, highlightCount);
});

test("highlighting does not join different fields or mark counts and line numbers", async () => {
  const { search, tree } = await panel([{ ...endpoints[0], label: "a", url: "/b", name: "ab1" }]);
  search("ab");
  assert.equal(tree.querySelectorAll(".route")[0].querySelectorAll("mark").length, 1);
  assert.equal(tree.querySelectorAll("mark")[0].textContent, "ab");
  search("1");
  assert.deepEqual(tree.querySelectorAll("mark").map((node) => node.textContent), ["1"]);
});

test("large cached scans keep hidden rows unrendered and rapid input debounced", async (t) => {
  const rows = Array.from({ length: 8896 }, (_, index) => ({
    ...endpoints[0], path: `/project/Goods${Math.floor(index / 4)}.java`, url: `${endpoints[0].url}/${index}`,
  }));
  const { search, input, flush, tree } = await panel(rows);
  const started = performance.now();
  search("gdsdtl", false);
  t.diagnostic(`8896 cached endpoints filtered in ${(performance.now() - started).toFixed(1)} ms (Node DOM stub)`);
  assert.equal(tree.querySelectorAll(".endpoint").length, 0);
  assert.equal(tree.querySelectorAll("mark").length, 0);
  const controller = tree.querySelectorAll(".tree-node").find((node) => !node.open);
  controller.open = true;
  controller.listeners.toggle();
  assert.equal(tree.querySelectorAll(".endpoint").length, 4);
  assert.equal(tree.querySelectorAll("mark").length, 20);
  const currentTree = tree.children[0];
  for (const query of ["g", "gd", "gds", "gdsdtl"]) input(query);
  assert.equal(tree.children[0], currentTree);
  flush();
  assert.notEqual(tree.children[0], currentTree);
  assert.equal(tree.querySelectorAll(".endpoint").length, 0);
});
