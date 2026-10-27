// GNOME Shell window reset geometry and Mutter operations.

export function centeredWindowFrame(workArea, defaultWidth, defaultHeight) {
    if (!workArea || !Number.isFinite(workArea.x) || !Number.isFinite(workArea.y) ||
        !Number.isFinite(workArea.width) || !Number.isFinite(workArea.height) ||
        workArea.width <= 0 || workArea.height <= 0 ||
        !Number.isFinite(defaultWidth) || !Number.isFinite(defaultHeight) ||
        defaultWidth <= 0 || defaultHeight <= 0)
        return null;

    const width = Math.min(defaultWidth, workArea.width);
    const height = Math.min(defaultHeight, workArea.height);
    return {
        x: Math.round(workArea.x + (workArea.width - width) / 2),
        y: Math.round(workArea.y + (workArea.height - height) / 2),
        width,
        height,
    };
}

export function resetWindowFrame(window, workArea, defaultWidth, defaultHeight) {
    const frame = centeredWindowFrame(workArea, defaultWidth, defaultHeight);
    if (!window || !frame)
        return null;

    if (window.maximized_horizontally || window.maximized_vertically)
        window.unmaximize();
    window.move_resize_frame(false, frame.x, frame.y, frame.width, frame.height);
    return frame;
}

export function windowMatchesResetFrame(window, frame, tolerance = 2) {
    if (!window || !frame || window.maximized_horizontally || window.maximized_vertically)
        return false;

    const actual = window.get_frame_rect();
    if (!actual)
        return false;
    return Math.abs(actual.x - frame.x) <= tolerance &&
        Math.abs(actual.y - frame.y) <= tolerance &&
        Math.abs(actual.width - frame.width) <= tolerance &&
        Math.abs(actual.height - frame.height) <= tolerance;
}
