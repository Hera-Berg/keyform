"""cmp.py REF KF OUT_DIFF: compares two RGBA renders (straight alpha)."""
import sys
import numpy as np
from PIL import Image
a = np.asarray(Image.open(sys.argv[1]).convert("RGBA")).astype(np.int32)
b = np.asarray(Image.open(sys.argv[2]).convert("RGBA")).astype(np.int32)
if a.shape != b.shape:
    print("size differs", a.shape, b.shape)
    h, w = min(a.shape[0], b.shape[0]), min(a.shape[1], b.shape[1])
    a, b = a[:h, :w], b[:h, :w]
# Compare premultiplied colour so invisible colour does not count.
pa = a[..., :3] * a[..., 3:4] // 255
pb = b[..., :3] * b[..., 3:4] // 255
d = np.abs(np.concatenate([pa - pb, a[..., 3:4] - b[..., 3:4]], axis=-1)).max(axis=-1)
print(f"pixels {d.size}, max diff {d.max()}, mean {d.mean():.3f}, >8: {(d > 8).sum()} ({100 * (d > 8).mean():.2f}%), >32: {(d > 32).sum()} ({100 * (d > 32).mean():.2f}%)")
vis = np.clip(d * 4, 0, 255).astype(np.uint8)
Image.fromarray(vis).save(sys.argv[3])
