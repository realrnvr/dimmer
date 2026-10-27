// Pure geometry/colour helpers shared by the GNOME Shell renderer.
//
// Keeping these functions free of shell imports lets them run under plain GJS in
// tests, without GNOME Shell or a compositor.

/**
 * Parse a canonical RRGGBB tint. Invalid values fall back to black, matching the
 * Rust model.
 *
 * @param {string} tint
 * @returns {[number, number, number]}
 */
export function parseTint(tint) {
    const value = String(tint ?? '').trim().replace(/^#/, '');
    if (!/^[0-9a-fA-F]{6}$/.test(value))
        return [0, 0, 0];

    const number = Number.parseInt(value, 16);
    return [(number >> 16) & 0xff, (number >> 8) & 0xff, number & 0xff];
}

/**
 * Build a CSS colour for one overlay. `dim` is the effective percentage 0–90.
 *
 * @param {string} tint
 * @param {number} dim
 * @returns {string}
 */
export function overlayColor(tint, dim) {
    const [red, green, blue] = parseTint(tint);
    const clamped = Math.max(0, Math.min(90, Number(dim) || 0));
    const alpha = (clamped / 100).toFixed(3);
    return `rgba(${red}, ${green}, ${blue}, ${alpha})`;
}

/**
 * Intersect two rectangles, returning null when they do not overlap.
 *
 * @param {{x: number, y: number, width: number, height: number}} a
 * @param {{x: number, y: number, width: number, height: number}} b
 * @returns {{x: number, y: number, width: number, height: number} | null}
 */
export function intersectRect(a, b) {
    const x = Math.max(a.x, b.x);
    const y = Math.max(a.y, b.y);
    const right = Math.min(a.x + a.width, b.x + b.width);
    const bottom = Math.min(a.y + a.height, b.y + b.height);
    if (right <= x || bottom <= y)
        return null;
    return {x, y, width: right - x, height: bottom - y};
}

/**
 * Split a monitor rectangle into up to four non-overlapping rectangles around a
 * hole (the non-dimmable settings window). When no hole overlaps the monitor the
 * full rectangle is returned. An empty array means the hole covers everything.
 *
 * @param {{x: number, y: number, width: number, height: number}} monitor
 * @param {{x: number, y: number, width: number, height: number} | null} hole
 * @returns {Array<{x: number, y: number, width: number, height: number}>}
 */
export function rectsForMonitor(monitor, hole) {
    if (!hole)
        return [monitor];

    const clipped = intersectRect(monitor, hole);
    if (!clipped)
        return [monitor];

    const monitorRight = monitor.x + monitor.width;
    const monitorBottom = monitor.y + monitor.height;
    const holeRight = clipped.x + clipped.width;
    const holeBottom = clipped.y + clipped.height;

    const candidates = [
        // Above the hole.
        {x: monitor.x, y: monitor.y, width: monitor.width, height: clipped.y - monitor.y},
        // Below the hole.
        {x: monitor.x, y: holeBottom, width: monitor.width, height: monitorBottom - holeBottom},
        // Left of the hole (same vertical band).
        {x: monitor.x, y: clipped.y, width: clipped.x - monitor.x, height: clipped.height},
        // Right of the hole (same vertical band).
        {x: holeRight, y: clipped.y, width: monitorRight - holeRight, height: clipped.height},
    ];

    return candidates.filter(rect => rect.width > 0 && rect.height > 0);
}

/**
 * Stable identity for the monitor layout. A change means actors must be rebuilt.
 *
 * @param {Array<object>} screens
 * @returns {string}
 */
export function layoutSignature(screens) {
    if (!Array.isArray(screens))
        return '';
    return screens
        .map(screen => {
            const geometry = screen.geometry ?? {};
            return [
                screen.id,
                screen.connector,
                geometry.x,
                geometry.y,
                geometry.width,
                geometry.height,
                screen.scale,
            ].join(':');
        })
        .join('|');
}
