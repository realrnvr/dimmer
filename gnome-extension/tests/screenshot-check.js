#!/usr/bin/env -S gjs -m
// Reports the brightest channel in the screen-identity label and the boundary
// strip of a Shell screenshot, so headless-shell.sh can prove neither is
// dimmed. Replaces the former PIL snippet; GdkPixbuf is already a GNOME Shell
// dependency.

import GdkPixbuf from "gi://GdkPixbuf";
import System from "system";

const [path] = ARGV;
if (!path) {
    printerr("usage: screenshot-check.js <png>");
    System.exit(2);
}

const pixbuf = GdkPixbuf.Pixbuf.new_from_file(path);
const pixels = pixbuf.get_pixels();
const stride = pixbuf.get_rowstride();
const channels = pixbuf.get_n_channels();

const width = pixbuf.get_width();
const height = pixbuf.get_height();

function maxChannel(x0, x1, y0, y1) {
    let max = 0;
    for (let y = y0; y < Math.min(y1, height); y++) {
        for (let x = x0; x < Math.min(x1, width); x++) {
            const offset = y * stride + x * channels;
            max = Math.max(max, pixels[offset], pixels[offset + 1], pixels[offset + 2]);
        }
    }
    return max;
}

const label = maxChannel(30, 150, 30, 190);
const boundary = maxChannel(0, 5, 200, 900);
print(`label max channel=${label} boundary max channel=${boundary}`);
