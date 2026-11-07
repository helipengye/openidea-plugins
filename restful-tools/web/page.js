(async () => {
  const catalog = Object.fromEntries(
    await Promise.all(
      ["en", "zh"].map(async (language) => {
        const response = await fetch(`./locales/${language}.json`);
        if (!response.ok) throw new Error(`Locale ${language}: HTTP ${response.status}`);
        return [language, await response.json()];
      }),
    ),
  );
  const $ = (id) => document.getElementById(id);
  const client = `${Date.now()}-${Math.random()}`;
  let language = navigator.language.startsWith("zh") ? "zh" : "en";
  let settings = { active: "local", environments: { local: { baseUrl: "http://localhost:8080" } } };
  let endpoints = [];
  let selected = null;
  let root = "";
  let scanning = false;
  let generation = null;
  let cancelRequested = false;
  const expanded = new Set(["project"]);
  let lastTreeRender = 0;
  let sending = false;
  let saving = false;
  let lastScan = null;
  let lastResponse = null;
  let notice = null;

  const t = (key) => catalog[language][key] || key;
  const format = (key, values = {}) => t(key).replace(/\{(\w+)\}/g, (_, name) => String(values[name] ?? ""));
  function post(message) {
    const text = JSON.stringify({ ...message, client, root: message.op === "ready" ? "" : root });
    if (new TextEncoder().encode(text).length > 900 * 1024) {
      sending = scanning = saving = false;
      pendingSettings = null;
      busy();
      showNotice("request_size", {}, true);
      return;
    }
    window.openidea.postMessage(text);
  }

  function showNotice(key, values = {}, error = false) {
    notice = { key, values, error };
    $("notice").textContent = format(key, values);
    $("notice").dataset.error = String(error);
  }

  function busy() {
    $("scan").disabled = scanning || sending || saving || !root;
    $("send").disabled = sending || scanning || saving || !root;
    $("navigate").disabled = selected === null || scanning || sending;
    $("save-environments").disabled = saving || sending || scanning;
    $("environment").disabled = saving || sending || scanning;
    $("send").textContent = t(sending ? "sending" : "send");
    $("scan").textContent = t(scanning ? "scanning" : "scan");
    $("cancel-scan").hidden = !scanning;
    $("cancel-scan").disabled = generation === null || cancelRequested;
  }

  function scanStatus() {
    $("scan-status").textContent = lastScan
      ? format("scanResult", {
          files: lastScan.files,
          total: lastScan.total_files,
          count: endpoints.length,
          skipped: lastScan.skipped,
        }) + (lastScan.limited ? " " + t(lastScan.limit === "memory" ? "indexLimited" : "fileQueryLimited") : "")
      : t("notScanned");
  }

  function responseStatus() {
    $("response-status").textContent = lastResponse
      ? `${lastResponse.status} · ${lastResponse.elapsed} ms · ${lastResponse.bytes} B${lastResponse.truncated ? " · " + t("previewLimited") : ""}`
      : t("noResponse");
  }

  function localize() {
    document.documentElement.lang = language;
    $("language").value = language;
    for (const element of document.querySelectorAll("[data-text]")) element.textContent = t(element.dataset.text);
    for (const element of document.querySelectorAll("[data-placeholder]"))
      element.placeholder = t(element.dataset.placeholder);
    for (const element of document.querySelectorAll("[data-label]"))
      element.setAttribute("aria-label", t(element.dataset.label));
    $("language").setAttribute("aria-label", t("language"));
    if (notice) showNotice(notice.key, notice.values, notice.error);
    scanStatus();
    responseStatus();
    renderTree();
    busy();
  }

  function renderTree() {
    const tree = $("tree");
    tree.replaceChildren();
    lastTreeRender = Date.now();
    const words = $("search").value.trim().toLowerCase().split(/\s+/).filter(Boolean);
    const modules = new Map();
    let matches = 0;
    endpoints.forEach((endpoint, index) => {
      const haystack =
        `${endpoint.module} ${endpoint.method} ${endpoint.url} ${endpoint.controller} ${endpoint.controller_label} ${endpoint.name} ${endpoint.label} ${endpoint.path}`.toLowerCase();
      if (!words.every((word) => haystack.includes(word))) return;
      matches++;
      if (!modules.has(endpoint.module)) modules.set(endpoint.module, { count: 0, controllers: new Map() });
      const module = modules.get(endpoint.module);
      module.count++;
      const key = `${endpoint.path}\n${endpoint.controller}`;
      if (!module.controllers.has(key)) module.controllers.set(key, []);
      module.controllers.get(key).push({ endpoint, index });
    });
    if (!modules.size) {
      const empty = document.createElement("p");
      empty.className = "muted";
      empty.textContent = t(lastScan ? "noMatches" : "notScanned");
      tree.append(empty);
      return;
    }

    function folder(key, label, title, populate, kind) {
      const group = document.createElement("details");
      group.className = "tree-node";
      group.open = (words.length > 0 && (kind !== "controller" || matches <= 200)) || expanded.has(key);
      const heading = document.createElement("summary");
      heading.append(icon(kind), document.createTextNode(label));
      heading.title = title;
      const children = document.createElement("div");
      children.className = "tree-children";
      let built = false;
      function update() {
        if (!words.length) {
          if (group.open) expanded.add(key);
          else expanded.delete(key);
        }
        if (group.open && !built) {
          populate(children);
          built = true;
        }
        if (!group.open && built) {
          children.replaceChildren();
          built = false;
        }
      }
      group.addEventListener("toggle", update);
      group.append(heading, children);
      update();
      return group;
    }

    function routes(children, rows) {
      for (const { endpoint, index } of rows) {
        const button = document.createElement("button");
        button.className = "endpoint";
        button.setAttribute("aria-pressed", String(selected === index));
        button.title = `${endpoint.path}:${endpoint.line + 1}`;
        const verb = document.createElement("span");
        verb.className = "verb";
        verb.textContent = endpoint.method;
        const route = document.createElement("span");
        route.className = "route";
        route.textContent = endpoint.label ? `${endpoint.label}  ${endpoint.url}` : endpoint.url;
        const name = document.createElement("small");
        name.textContent = `${endpoint.name} · ${endpoint.line + 1}`;
        route.append(name);
        button.append(verb, route);
        button.addEventListener("click", () => {
          if (sending || scanning) return;
          selected = index;
          $("method").value = endpoint.method === "ANY" ? "GET" : endpoint.method;
          $("url").value = "{{baseUrl}}" + endpoint.url;
          for (const item of tree.querySelectorAll(".endpoint"))
            item.setAttribute("aria-pressed", String(item === button));
          busy();
        });
        button.addEventListener("dblclick", () => {
          if (!sending && !scanning) post({ op: "navigate", index, generation });
        });
        children.append(button);
      }
    }

    const project = root.split(/[\\/]/).filter(Boolean).pop() || "RESTful Tools";
    tree.append(
      folder(
        "project",
        `${project} · ${format("treeTotal", { count: matches })}`,
        root,
        (children) => {
          for (const [moduleName, module] of [...modules].sort(([a], [b]) => a.localeCompare(b))) {
            const name = moduleName === "." ? project : moduleName.split("/").pop();
            children.append(
              folder(
                `module:${moduleName}`,
                `${name} (${module.count})`,
                moduleName,
                (controllers) => {
                  for (const [key, rows] of module.controllers) {
                    const first = rows[0].endpoint;
                    controllers.append(
                      folder(
                        `controller:${key}`,
                        `${first.controller_label || first.controller} (${rows.length})`,
                        `${first.controller} · ${first.path}`,
                        (items) => routes(items, rows),
                        "controller",
                      ),
                    );
                  }
                },
                "module",
              ),
            );
          }
        },
        "project",
      ),
    );
  }

  function icon(kind) {
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", "0 0 16 16");
    svg.setAttribute("aria-hidden", "true");
    svg.classList.add("tree-icon", `tree-icon-${kind}`);
    const path = document.createElementNS(svg.namespaceURI, "path");
    path.setAttribute(
      "d",
      "M6.7 2.5H2a.5.5 0 0 0-.5.5v10a.5.5 0 0 0 .5.5h12a.5.5 0 0 0 .5-.5V5a.5.5 0 0 0-.5-.5H8.7L7.1 2.7a.5.5 0 0 0-.4-.2Z",
    );
    path.setAttribute("fill", "none");
    path.setAttribute("stroke", "currentColor");
    svg.append(path);
    return svg;
  }

  function startScan() {
    scanning = true;
    generation = null;
    cancelRequested = false;
    selected = null;
    endpoints = [];
    lastScan = null;
    renderTree();
    scanStatus();
    showNotice("scanning");
    busy();
    post({ op: "scan" });
  }

  function environments() {
    $("environment").replaceChildren();
    for (const name of Object.keys(settings.environments)) {
      const option = document.createElement("option");
      option.value = name;
      option.textContent = name;
      $("environment").append(option);
    }
    $("environment").value = settings.active;
    $("environment-json").value = JSON.stringify(settings.environments, null, 2);
  }

  let pendingSettings = null;
  $("save-environments").addEventListener("click", () => {
    try {
      const data = JSON.parse($("environment-json").value);
      if (!data || Array.isArray(data) || typeof data !== "object" || !Object.keys(data).length) throw new Error();
      for (const variables of Object.values(data)) {
        if (
          !variables ||
          Array.isArray(variables) ||
          typeof variables !== "object" ||
          Object.values(variables).some((value) => typeof value !== "string")
        )
          throw new Error();
      }
      pendingSettings = {
        active: Object.hasOwn(data, settings.active) ? settings.active : Object.keys(data)[0],
        environments: data,
      };
      saving = true;
      busy();
      post({ op: "save", settings: pendingSettings });
    } catch {
      showNotice("environmentFormat", {}, true);
    }
  });

  $("environment").addEventListener("change", () => {
    pendingSettings = { ...settings, active: $("environment").value };
    saving = true;
    busy();
    post({ op: "save", settings: pendingSettings });
  });
  $("environments").addEventListener("click", () => {
    $("environment-editor").hidden = !$("environment-editor").hidden;
  });
  $("close-environments").addEventListener("click", () => {
    $("environment-editor").hidden = true;
  });
  $("language").addEventListener("change", () => {
    language = $("language").value;
    localize();
  });
  let searchTimer;
  $("search").addEventListener("input", () => {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(renderTree, 120);
  });
  $("navigate").addEventListener("click", () => {
    if (selected !== null) post({ op: "navigate", index: selected, generation });
  });
  $("scan").addEventListener("click", startScan);
  $("cancel-scan").addEventListener("click", () => {
    cancelRequested = true;
    busy();
    post({ op: "scan-cancel", generation });
  });
  $("request").addEventListener("submit", (event) => {
    event.preventDefault();
    if (sending || scanning || saving || !root) return;
    sending = true;
    lastResponse = null;
    $("response-headers").textContent = "";
    $("response-body").textContent = "";
    responseStatus();
    showNotice("sending");
    busy();
    post({
      op: "send",
      settings,
      draft: { method: $("method").value, url: $("url").value, headers: $("headers").value, body: $("body").value },
    });
  });

  window.openidea.onMessage((text) => {
    let message;
    try {
      message = JSON.parse(text);
    } catch {
      return;
    }
    if (message.client !== client && !(message.client === "" && message.op.startsWith("scan") && message.root === root))
      return;
    switch (message.op) {
      case "ready":
        if (root && root !== message.root) {
          $("url").value = "{{baseUrl}}/";
          $("headers").value = "";
          $("body").value = "";
          lastResponse = null;
          $("response-headers").textContent = "";
          $("response-body").textContent = "";
          responseStatus();
        }
        root = message.root;
        settings = message.settings;
        $("project").textContent = root;
        $("project").title = root;
        environments();
        busy();
        break;
      case "scan-begin":
        generation = message.generation;
        root = message.root;
        endpoints = [];
        lastScan = null;
        selected = null;
        break;
      case "scan-page":
        if (message.generation !== generation) return;
        endpoints.length = message.offset;
        for (const endpoint of message.endpoints) endpoints.push(endpoint);
        break;
      case "scan":
        if (message.scan && message.scan.generation !== generation) return;
        scanning = message.continue && !cancelRequested;
        root = message.root;
        lastScan = message.scan;
        if (!scanning || Date.now() - lastTreeRender > 300) renderTree();
        scanStatus();
        busy();
        showNotice(
          !lastScan
            ? "notScanned"
            : lastScan.cancelled
              ? "scanCancelled"
              : scanning
                ? "scanning"
                : lastScan.done
                  ? "scanComplete"
                  : "scanPaused",
        );
        if (scanning) post({ op: "scan-next", generation });
        break;
      case "saved":
        saving = false;
        if (pendingSettings) {
          settings = pendingSettings;
          pendingSettings = null;
          environments();
        }
        busy();
        showNotice("saved");
        break;
      case "response":
        sending = false;
        lastResponse = message;
        $("response-headers").textContent = message.headers.map(([name, value]) => `${name}: ${value}`).join("\n");
        try {
          $("response-body").textContent = JSON.stringify(JSON.parse(message.body), null, 2);
        } catch {
          $("response-body").textContent = message.body;
        }
        responseStatus();
        busy();
        showNotice("requestComplete");
        break;
      case "error": {
        if (message.request.startsWith("scan") && message.generation != null && message.generation !== generation)
          return;
        if (message.request.startsWith("scan")) {
          scanning = false;
          cancelRequested = false;
          renderTree();
        }
        if (message.request === "send") sending = false;
        if (message.request === "save") {
          saving = false;
          pendingSettings = null;
        }
        busy();
        const [code, ...detail] = message.code.split(":");
        if (code === "project_changed") post({ op: "ready" });
        showNotice(
          catalog[language][code] ? code : "failed",
          { detail: message.detail || detail.join(":") || message.code },
          true,
        );
        break;
      }
    }
  });
  localize();
  post({ op: "ready" });
})().catch((error) => {
  document.getElementById("notice").textContent = `RESTful Tools: ${error.message}`;
  document.getElementById("notice").dataset.error = "true";
});
