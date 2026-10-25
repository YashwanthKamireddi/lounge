"""Freeze the bundled variable font at the UI's weights and optical sizes.

With a second argument, that variable font (Apple's SF Pro, kept untracked in
local/fonts) is first cut to the text Luma shows: Latin and Latin Extended,
at its regular width, without the symbol glyphs it also carries.
"""
import sys
import json
from pathlib import Path
from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

directory = Path(sys.argv[1])
if len(sys.argv) > 2:
    source = TTFont(sys.argv[2])
    keep = set(range(0x20, 0x250)) | set(range(0x250, 0x2B0)) | set(range(0x2B0, 0x370))
    keep |= set(range(0x1E00, 0x1F00)) | set(range(0x2000, 0x2070)) | set(range(0x20A0, 0x20D0))
    keep |= set(range(0x2100, 0x2200)) | set(range(0x2190, 0x2200)) | {0x2022, 0x25CF, 0xFEFF}
    options = subset.Options()
    options.layout_features = ["calt", "ccmp", "dnom", "frac", "kern", "locl", "mark", "mkmk", "numr", "pnum", "tnum"]
    options.name_IDs = ["*"]
    options.notdef_outline = True
    subsetter = subset.Subsetter(options=options)
    subsetter.populate(unicodes=keep & set(source.getBestCmap()))
    subsetter.subset(source)
    if "fvar" in source and "wdth" in {axis.axisTag for axis in source["fvar"].axes}:
        source = instantiateVariableFont(source, {"wdth": 100})
    source.save(directory / "font.ttf")
font = TTFont(directory / "font.ttf")
axes = {axis.axisTag for axis in font["fvar"].axes} if "fvar" in font else set()
metrics = {}
for weight in (350, 400, 450, 500, 550, 600, 650, 800):
    for display in (False, True):
        values = {"wght": weight} if "wght" in axes else {}
        if "opsz" in axes:
            # Text at the axis's small end, display (titles) at its large end.
            opsz = next(axis for axis in font["fvar"].axes if axis.axisTag == "opsz")
            values["opsz"] = opsz.maxValue if display else opsz.minValue
        instance = instantiateVariableFont(font, values, inplace=False) if axes else font
        instance.save(directory / f"font-{'display-' if display else ''}{weight}.ttf")
        units = instance["head"].unitsPerEm
        advances = instance["hmtx"].metrics
        metrics[f"{'display-' if display else ''}{weight}"] = {
            chr(codepoint): advances[glyph][0] / units
            for codepoint, glyph in instance.getBestCmap().items()
        }
(directory / "font-metrics.json").write_text(json.dumps(metrics, ensure_ascii=False), encoding="utf8")
