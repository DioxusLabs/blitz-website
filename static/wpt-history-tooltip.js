// Hover tooltip for WPT history charts (progressive enhancement): reads run
// data from each chart's JSON blob and shows the nearest run of the nearest
// line: its revision, commit message, and pass percentage. Each series has
// its own list of runs (lines for different products are recorded on
// different dates).
document.querySelectorAll("script[data-wpt-history-data]").forEach(function (dataEl) {
    if (dataEl.dataset.tooltipInit) return;
    dataEl.dataset.tooltipInit = "1";
    var container = dataEl.parentElement;
    var svg = container.querySelector("svg");
    var data = JSON.parse(dataEl.textContent);
    if (!svg || !data.series.some(function (s) { return s.runs.length; })) return;

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

    var px = data.plot[0], py = data.plot[1], pw = data.plot[2], ph = data.plot[3];
    var xRange = data.xMax - data.xMin;

    function esc(s) {
        return s.replace(/&/g, "&amp;").replace(/</g, "&lt;");
    }

    function screenX(run) { return px + ((run.x - data.xMin) / xRange) * pw; }
    function screenY(s, run) { return py + (1 - run.v[0] / s.total) * ph; }

    // Nearest hoverable run of a series to the x position `x` (in data
    // units); runs before `s.first` only serve as deltas
    function nearest(s, x) {
        var runs = s.runs, lo = s.first, hi = runs.length - 1;
        while (lo < hi) {
            var mid = (lo + hi) >> 1;
            if (runs[mid].x < x) lo = mid + 1; else hi = mid;
        }
        if (lo > s.first && Math.abs(runs[lo - 1].x - x) < Math.abs(runs[lo].x - x)) lo--;
        return lo;
    }

    function hide() {
        tip.style.display = "none";
        guide.style.display = "none";
        dot.style.display = "none";
    }

    // Only show a series whose line is within this distance (in viewBox
    // units) of the cursor
    var Y_THRESHOLD = 12;

    // Distance from the cursor to the series' drawn line. Measured to the
    // polyline's segments (2D point-to-segment distance) so steep,
    // near-vertical jumps are hoverable anywhere along their length, not
    // just near their endpoints.
    function distToSegment(x, y, x1, y1, x2, y2) {
        var dx = x2 - x1, dy = y2 - y1;
        var len2 = dx * dx + dy * dy;
        var t = len2 ? ((x - x1) * dx + (y - y1) * dy) / len2 : 0;
        t = Math.max(0, Math.min(1, t));
        return Math.hypot(x - (x1 + t * dx), y - (y1 + t * dy));
    }
    function seriesDist(s, vx, vy) {
        var runs = s.runs;
        // Consider only segments within a horizontal window of the cursor
        var windowPx = Y_THRESHOLD;
        var dist = Infinity, prev = null;
        for (var j = s.first; j < runs.length; j++) {
            var run = runs[j];
            if (run.v == null) continue;
            var x = screenX(run), y = screenY(s, run);
            if (prev && x >= vx - windowPx && screenX(prev) <= vx + windowPx) {
                dist = Math.min(dist, distToSegment(vx, vy, screenX(prev), screenY(s, prev), x, y));
            } else if (!prev && Math.abs(x - vx) <= windowPx) {
                dist = Math.min(dist, Math.hypot(x - vx, y - vy));
            }
            if (x > vx + windowPx) break;
            prev = run;
        }
        return dist;
    }

    svg.addEventListener("mousemove", function (ev) {
        var rect = svg.getBoundingClientRect();
        var scale = data.width / rect.width;
        var vx = (ev.clientX - rect.left) * scale;
        var vy = (ev.clientY - rect.top) * scale;
        if (vx < px || vx > px + pw) { hide(); return; }

        // Pick the single series whose drawn line is nearest the cursor
        var s = null, bestDist = Y_THRESHOLD;
        for (var i = 0; i < data.series.length; i++) {
            if (!data.series[i].total) continue;
            var dist = seriesDist(data.series[i], vx, vy);
            if (dist < bestDist) { s = data.series[i]; bestDist = dist; }
        }
        if (!s) { hide(); return; }

        var runIdx = nearest(s, data.xMin + ((vx - px) / pw) * xRange);
        if (s.runs[runIdx].v == null) { hide(); return; }

        // Prefer a nearby run whose value actually changed over the
        // strictly-nearest run
        var SNAP_PX = 10;
        function hasChange(i) {
            var cur = s.runs[i].v;
            if (cur == null) return false;
            var p = i > 0 ? s.runs[i - 1].v : null;
            return p == null || cur[0] !== p[0] || cur[1] !== p[1];
        }
        var snapped = -1, snappedDist = SNAP_PX;
        for (var j = s.first; j < s.runs.length; j++) {
            var d = Math.abs(screenX(s.runs[j]) - vx);
            if (d <= snappedDist && hasChange(j)) { snapped = j; snappedDist = d; }
        }
        if (snapped >= 0 && !hasChange(runIdx)) runIdx = snapped;
        var run = s.runs[runIdx];
        var prev = runIdx > 0 ? s.runs[runIdx - 1] : null;

        var runVx = screenX(run);
        guide.setAttribute("x1", runVx);
        guide.setAttribute("x2", runVx);
        guide.style.display = "";

        dot.setAttribute("cx", runVx);
        dot.setAttribute("cy", screenY(s, run));
        dot.setAttribute("fill", s.color);
        dot.style.display = "";

        var html = "<div style='font-weight:bold'>" + esc(run.rev) + " (" + esc(run.d) + ")</div>";
        if (run.msg) {
            html += "<div style='margin-bottom:4px;white-space:nowrap;overflow:hidden;" +
                "text-overflow:ellipsis'>" + esc(run.msg) + "</div>";
        }
        var pass = run.v[0], total = s.total;
        html += "<div><span style='color:" + s.color + "'>\u25CF</span> " +
            esc(s.name) + ": " + (100 * pass / total).toFixed(1) + "% (" +
            pass.toLocaleString() + "/" + total.toLocaleString() + ")</div>";

        // Change relative to the previous run
        if (prev && prev.v != null) {
            var dPass = pass - prev.v[0];
            var dPct = 100 * (dPass / total);
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
