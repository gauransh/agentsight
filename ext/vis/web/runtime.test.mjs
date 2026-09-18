import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createContext, SourceTextModule, SyntheticModule } from "node:vm";
import * as nebula from "./repository-nebula.js";

// Exercise the actual browser runtime with the real nebula model. Only browser
// rendering/media APIs are replaced; initialization, controls, graph contents,
// provenance, and replay state all run through production code.
async function runtime() {
  const nodes = new Map();
  function node(id) {
    if (!nodes.has(id)) nodes.set(id, {
      textContent: id === "play" ? "▶" : "", attributes: {}, listeners: {},
      classList: { toggle() {} },
      setAttribute(key, value) { this.attributes[key] = value; },
      addEventListener(key, listener) { this.listeners[key] = listener; },
    });
    return nodes.get(id);
  }
  const frames = new Map();
  let nextFrame = 0;
  let option;
  const context = createContext({
    document: { getElementById: node, querySelector: node },
    location: { search: "" }, window: {}, URLSearchParams,
    performance: { now: () => 0 },
    requestAnimationFrame: (callback) => { frames.set(++nextFrame, callback); return nextFrame; },
    cancelAnimationFrame: (id) => frames.delete(id),
  });
  const modules = {
    "echarts/core": {
      use() {},
      init: () => ({ setOption(value) { option = value; }, getZr: () => ({ flush() {} }) }),
    },
    "echarts/charts": { ScatterChart: {} },
    "echarts/components": { GraphicComponent: {}, GridComponent: {}, TooltipComponent: {} },
    "echarts/renderers": { CanvasRenderer: {}, SVGRenderer: {} },
    mediabunny: { BufferTarget: {}, CanvasSource: {}, Mp4OutputFormat: {}, Output: {}, QUALITY_HIGH: 1, canEncodeVideo() {} },
    "./repository-nebula.js": nebula,
  };
  const source = new SourceTextModule(await readFile(new URL("./runtime.js", import.meta.url), "utf8"), { context });
  await source.link((name) => {
    const exports = modules[name];
    assert.ok(exports, `unexpected browser dependency: ${name}`);
    return new SyntheticModule(Object.keys(exports), function () {
      for (const [key, value] of Object.entries(exports)) this.setExport(key, value);
    }, { context });
  });
  await source.evaluate();
  return {
    initialize: context.AgentVis.initialize,
    node,
    option: () => option,
    tick(now) {
      const [id, callback] = frames.entries().next().value;
      frames.delete(id);
      callback(now);
    },
  };
}

function event(index, path, extra = {}) {
  return {
    id: `event-${index}`, ts_ms: 1_780_000_000_000 + index * 1_000,
    session_id: "claude:session", vendor: "claude", tool_name: path ? "Read" : "Bash",
    category: path ? "file" : "shell", command_name: "", status: "ok",
    actions: path ? [{ path, access: "read" }] : [], ...extra,
  };
}

function data(events, meta = {}) {
  return {
    meta: { repository: "fixture", endpoint_revision: "revision", session_scope: "single_session",
      window_start_ms: events[0]?.ts_ms ?? 0, window_end_ms: events.at(-1)?.ts_ms ?? 0, ...meta },
    events, commits: [],
  };
}

const series = (option, id) => option.series.find(row => row.id === id).data;
const graphicText = (option) => option.graphic.flatMap(group => group.children ?? []).map(row => row.style?.text ?? "").join("\n");

test("initial load shows all recorded files; Play rewinds and replays the actual events", async () => {
  const app = await runtime();
  app.initialize(data([event(0), event(1, "src/main.rs"), event(2, "tests/main.rs")]));
  assert.equal(app.node("timeline").value, "2");
  assert.equal(series(app.option(), "files").length, 2);
  assert.equal(app.node("play").disabled, false);
  app.node("play").listeners.click();
  assert.equal(app.node("timeline").value, "0");
  assert.equal(series(app.option(), "files").length, 0);
  app.tick(125);
  assert.equal(series(app.option(), "files").length, 1);
  app.tick(250);
  assert.equal(series(app.option(), "files").length, 2);
  assert.equal(app.node("play").textContent, "▶");
});

test("one event shows its file immediately and disables nonexistent playback", async () => {
  const app = await runtime();
  app.initialize(data([event(0, "src/main.rs")]));
  assert.equal(series(app.option(), "files").length, 1);
  assert.equal(app.node("play").disabled, true);
  assert.equal(app.node("timeline").disabled, true);
});

test("tool-only sessions preserve their real timeline and explain the absence of file stars", async () => {
  const app = await runtime();
  app.initialize(data([event(0), event(1)]));
  assert.equal(series(app.option(), "files").length, 0);
  assert.match(graphicText(app.option()), /2 recorded tool events/);
  assert.match(graphicText(app.option()), /No repository file activity is available/);
  assert.equal(app.node("play").disabled, false);
  assert.equal(app.node("timeline").value, "1");
});

test("zero events have an explicit native canvas state without invented dates or files", async () => {
  const app = await runtime();
  app.initialize(data([]));
  assert.equal(series(app.option(), "files").length, 0);
  assert.match(graphicText(app.option()), /No session activity is available/);
  assert.equal(app.node("play").disabled, true);
  assert.equal(app.node("timeline").disabled, true);
  assert.equal(app.node("cursor-label").textContent, "No session activity");
  assert.doesNotMatch(app.node("provenance").textContent, /1970/);
  assert.match(app.node("chart").attributes["aria-label"], /No session activity/);
});

test("recorded opens render real file stars and denials without native tool or write claims", async () => {
  const app = await runtime();
  const recorded = (index, path, status) => event(index, path, {
    vendor: "recorded-activity", session_id: "run-id", tool_name: "FileOpenReadWrite", status,
    actions: [{ path, access: "open" }],
  });
  app.initialize(data([recorded(0, "src/main.rs", "allowed"), recorded(1, "secrets/token", "denied")], {
    activity_source: "recorded_file_activity", activity_partial: true,
    session_scope: "operation_recorded_activity", run_id: "run-id",
  }));
  const option = app.option();
  const files = series(option, "files");
  assert.equal(files.length, 2);
  assert.equal(files[0].visits, 1);
  assert.equal(series(option, "write-ripples").length, 0);
  assert.equal(series(option, "read-rings").length, 0);
  assert.equal(series(option, "lifecycle").length, 0);
  assert.ok(series(option, "open-rings").some(row => row.itemStyle.borderColor === "#ff647c"));
  assert.match(graphicText(option), /denied/);
  assert.match(graphicText(option), /does not establish a read, write, or file change/);
  const tooltip = option.tooltip.formatter({ data: files.find(row => row.path === "secrets/token") });
  assert.match(tooltip, /denied · operation run-id/);
  assert.doesNotMatch(tooltip, /agent-session|direct tool|1 sessions/);
  assert.match(app.node("provenance").textContent, /recorded file-open activity/);
  assert.match(app.node("provenance").textContent, /partial audit records/);
});

test("an empty recorded-activity trace keeps its own source and no native session claim", async () => {
  const app = await runtime();
  app.initialize(data([], {
    activity_source: "recorded_file_activity", session_scope: "operation_recorded_activity",
    activity_partial: true,
  }));
  assert.match(graphicText(app.option()), /No recorded file activity is available/);
  assert.equal(app.node("cursor-label").textContent, "No recorded file activity");
  assert.match(app.node("provenance").textContent, /this operation's recorded file activity/);
  assert.doesNotMatch(app.node("provenance").textContent, /this session|1970/);
  assert.equal(app.node("play").disabled, true);
});
