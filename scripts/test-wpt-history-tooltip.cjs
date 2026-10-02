const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const source = fs.readFileSync(path.join(__dirname, "../static/wpt-history-tooltip.js"), "utf8");

function run(x, pass, total) {
    return { x, v: [pass, total], rev: "rev-" + x, d: "2026-01-01", msg: null };
}

function setup(series, xMin = 0, xMax = 1) {
    const children = [];
    const handlers = {};
    function element() {
        return {
            style: {},
            attributes: {},
            offsetWidth: 260,
            setAttribute(name, value) { this.attributes[name] = value; },
        };
    }
    const rect = { left: 0, top: 0, width: 900 };
    const svg = {
        appendChild(child) { children.push(child); },
        addEventListener(name, handler) { handlers[name] = handler; },
        getBoundingClientRect() { return rect; },
    };
    const container = {
        clientWidth: 900,
        appendChild(child) { children.push(child); },
        querySelector() { return svg; },
        getBoundingClientRect() { return rect; },
    };
    const data = {
        dataset: {},
        parentElement: container,
        textContent: JSON.stringify({
            width: 900, plot: [50, 25, 765, 235], xMin, xMax, series,
        }),
    };
    vm.runInNewContext(source, {
        document: {
            querySelectorAll() { return [data]; },
            createElement: element,
            createElementNS: element,
        },
    });
    return {
        tip: children[0],
        dot: children[2],
        hover(x, y) { handlers.mousemove({ clientX: x, clientY: y }); },
        leave() { handlers.mouseleave(); },
    };
}

function series(metric, total, runs) {
    return { name: "Blitz", color: "#000000", metric, total, first: 0, runs };
}

test("total hover uses the count axis and reports count changes", () => {
    const runs = [run(0, 50, 100), run(1, 100, 200)];
    const chart = setup([series("percent", 1000, runs), series("total", 250, runs)]);
    chart.hover(815, 72);
    assert.equal(chart.tip.style.display, "block");
    assert.match(chart.tip.innerHTML, /200 total subtests/);
    assert.match(chart.tip.innerHTML, /Change: \+100 subtests/);
    assert.equal(chart.dot.attributes.cy, 72);
    chart.leave();
    assert.equal(chart.tip.style.display, "none");
});

test("percentage hover retains the fixed denominator and score deltas", () => {
    const runs = [run(0, 50, 100), run(1, 100, 200)];
    const chart = setup([series("percent", 1000, runs), series("total", 250, runs)]);
    chart.hover(815, 236.5);
    assert.match(chart.tip.innerHTML, /10\.0% \(100\/1,000\)/);
    assert.match(chart.tip.innerHTML, /Change: \+50 \(\+5\.00%\)/);
    assert.equal(chart.dot.attributes.cy, 236.5);
});

test("total hover reports decreases without treating them as score regressions", () => {
    const chart = setup([series("total", 250, [run(0, 100, 200), run(1, 50, 100)])]);
    chart.hover(815, 166);
    assert.match(chart.tip.innerHTML, /100 total subtests/);
    assert.match(chart.tip.innerHTML, /Change: -100 subtests/);
    assert.doesNotMatch(chart.tip.innerHTML, /#c62828/);
});

test("single-run and missing-data charts avoid invalid tooltip coordinates", () => {
    const chart = setup([series("total", 250, [run(1, 100, 200)])], 1, 1);
    chart.hover(50, 72);
    assert.match(chart.tip.innerHTML, /200 total subtests/);
    assert.equal(chart.dot.attributes.cx, 50);
    assert.equal(chart.dot.attributes.cy, 72);
    assert.doesNotMatch(chart.tip.innerHTML, /Change:/);

    const missing = setup([series("total", 250, [{ ...run(0, 0, 0), v: null }])]);
    missing.hover(50, 72);
    assert.equal(missing.tip.style.display, "none");
});

test("zero subtest totals are valid counts, not missing runs", () => {
    const chart = setup([series("total", 250, [run(0, 50, 100), run(1, 0, 0)])]);
    chart.hover(815, 260);
    assert.match(chart.tip.innerHTML, /0 total subtests/);
    assert.match(chart.tip.innerHTML, /Change: -100 subtests/);
    assert.equal(chart.dot.attributes.cy, 260);
});
