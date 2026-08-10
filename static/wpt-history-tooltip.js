// Hover tooltip for WPT history charts (progressive enhancement): reads run
// data from each chart's JSON blob and shows the nearest run's commit id,
// commit message, and pass percentages for the nearest line.
document.querySelectorAll("script[data-wpt-history-data]").forEach(function (dataEl) {
    if (dataEl.dataset.tooltipInit) return;
    dataEl.dataset.tooltipInit = "1";
    var container = dataEl.parentElement;
    var svg = container.querySelector("svg");
    var data = JSON.parse(dataEl.textContent);
    if (!svg || !data.runs.length) return;

    var tip = document.createElement("div");
    tip.style.cssText =
        "position:absolute;pointer-events:none;display:none;background:rgba(255,255,255,0.96);" +
        "border:1px solid #999;border-radius:4px;padding:6px 8px;font:12px sans-serif;" +
        "box-shadow:0 1px 4px rgba(0,0,0,0.25);z-index:10;width:260px;box-sizing:border-box";
    container.appendChild(tip);

    var guide = document.createElementNS("http://www.w3.org/2000/svg", "line");
    guide.setAttribute("stroke", "#888");
    guide.setAttribute("stroke-dasharray", "3,3");
    guide.setAttribute("y1", data.plot[1]);
    guide.setAttribute("y2", data.plot[1] + data.plot[3]);
    guide.style.display = "none";
    svg.appendChild(guide);

    var dot = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    dot.setAttribute("r", 4);
    dot.setAttribute("stroke", "white");
    dot.setAttribute("stroke-width", 1.5);
    dot.style.display = "none";
    svg.appendChild(dot);

    function esc(s) {
        return s.replace(/&/g, "&amp;").replace(/</g, "&lt;");
    }

    // Nearest hoverable run (runs before data.first only serve as deltas)
    function nearest(x) {
        var runs = data.runs, lo = data.first, hi = runs.length - 1;
        while (lo < hi) {
            var mid = (lo + hi) >> 1;
            if (runs[mid].x < x) lo = mid + 1; else hi = mid;
        }
        if (lo > data.first && Math.abs(runs[lo - 1].x - x) < Math.abs(runs[lo].x - x)) lo--;
        return lo;
    }

    function hide() {
        tip.style.display = "none";
        guide.style.display = "none";
        dot.style.display = "none";
    }

    // Only show the series whose line is within this vertical distance
    // (in viewBox units) of the cursor
    var Y_THRESHOLD = 12;

    svg.addEventListener("mousemove", function (ev) {
        var rect = svg.getBoundingClientRect();
        var scale = data.width / rect.width;
        var vx = (ev.clientX - rect.left) * scale;
        var vy = (ev.clientY - rect.top) * scale;
        var px = data.plot[0], py = data.plot[1], pw = data.plot[2], ph = data.plot[3];
        if (vx < px || vx > px + pw) { hide(); return; }

        var xRange = data.xMax - data.xMin;
        var runIdx = nearest(data.xMin + ((vx - px) / pw) * xRange);
        var run = data.runs[runIdx];
        var prev = runIdx > 0 ? data.runs[runIdx - 1] : null;

        // Pick the single series whose line is vertically nearest the cursor
        var best = -1, bestDist = Y_THRESHOLD;
        for (var i = 0; i < data.series.length; i++) {
            if (run.v[i] == null) continue;
            var sy = py + (1 - (run.v[i][0] / run.v[i][1])) * ph;
            var dist = Math.abs(sy - vy);
            if (dist < bestDist) { best = i; bestDist = dist; }
        }
        if (best < 0) { hide(); return; }

        // Prefer a nearby commit whose value actually changed for this
        // series over the strictly-nearest commit
        var SNAP_PX = 10;
        function screenX(i) { return px + ((data.runs[i].x - data.xMin) / xRange) * pw; }
        function hasChange(i) {
            var cur = data.runs[i].v[best];
            if (cur == null) return false;
            var p = i > 0 ? data.runs[i - 1].v[best] : null;
            return p == null || cur[0] !== p[0] || cur[1] !== p[1];
        }
        var snapped = -1, snappedDist = SNAP_PX;
        for (var j = data.first; j < data.runs.length; j++) {
            var d = Math.abs(screenX(j) - vx);
            if (d <= snappedDist && hasChange(j)) { snapped = j; snappedDist = d; }
        }
        if (snapped >= 0 && !hasChange(runIdx)) {
            runIdx = snapped;
            run = data.runs[runIdx];
            prev = runIdx > 0 ? data.runs[runIdx - 1] : null;
        }

        var runVx = px + ((run.x - data.xMin) / xRange) * pw;
        guide.setAttribute("x1", runVx);
        guide.setAttribute("x2", runVx);
        guide.style.display = "";

        dot.setAttribute("cx", runVx);
        dot.setAttribute("cy", py + (1 - (run.v[best][0] / run.v[best][1])) * ph);
        dot.setAttribute("fill", data.series[best].color);
        dot.style.display = "";

        var html = "<div style='font-weight:bold'>" + esc(run.sha.slice(0, 9)) + " (" + esc(run.d) + ")</div>";
        if (run.msg) {
            html += "<div style='margin-bottom:4px;white-space:nowrap;overflow:hidden;" +
                "text-overflow:ellipsis'>" + esc(run.msg) + "</div>";
        }
        var pass = run.v[best][0], total = run.v[best][1];
        html += "<div><span style='color:" + data.series[best].color + "'>\u25CF</span> " +
            esc(data.series[best].name) + ": " + (100 * pass / total).toFixed(1) + "% (" +
            pass.toLocaleString() + "/" + total.toLocaleString() + ")</div>";

        // Change relative to the previous run
        if (prev && prev.v[best] != null) {
            var dPass = pass - prev.v[best][0];
            var dPct = 100 * (pass / total - prev.v[best][0] / prev.v[best][1]);
            var sign = dPass > 0 ? "+" : "";
            var color = dPass > 0 ? "#2e7d32" : (dPass < 0 ? "#c62828" : "#666");
            html += "<div style='color:" + color + "'>Change: " + sign +
                dPass.toLocaleString() + " (" + sign + dPct.toFixed(2) + "%)</div>";
        }
        tip.innerHTML = html;
        tip.style.display = "block";

        var crect = container.getBoundingClientRect();
        var cx = ev.clientX - crect.left, cy = ev.clientY - crect.top;
        var left = cx + 14;
        if (left + tip.offsetWidth > container.clientWidth) left = cx - tip.offsetWidth - 14;
        tip.style.left = left + "px";
        tip.style.top = (cy + 14) + "px";
    });
    svg.addEventListener("mouseleave", hide);
});
