"""Deterministic noisy-digit dataset for the off-chain PyTorch experiments.

Each sample is an 8x4 glyph of a decimal digit (32 int8 pixels) with
seeded noise and flipped pixels. Generation is pure Python with a fixed
64-bit LCG, so every platform produces the identical dataset, and the
inputs already lie in the QMLP-V1 int8 range.
"""

from __future__ import annotations

GLYPHS = (
    (".##.", "#..#", "#..#", "#..#", "#..#", "#..#", "#..#", ".##."),
    ("..#.", ".##.", "..#.", "..#.", "..#.", "..#.", "..#.", ".###"),
    (".##.", "#..#", "...#", "..#.", ".#..", "#...", "#...", "####"),
    ("###.", "...#", "...#", ".##.", "...#", "...#", "...#", "###."),
    ("#..#", "#..#", "#..#", "####", "...#", "...#", "...#", "...#"),
    ("####", "#...", "#...", "###.", "...#", "...#", "#..#", ".##."),
    (".##.", "#...", "#...", "###.", "#..#", "#..#", "#..#", ".##."),
    ("####", "...#", "...#", "..#.", "..#.", ".#..", ".#..", ".#.."),
    (".##.", "#..#", "#..#", ".##.", "#..#", "#..#", "#..#", ".##."),
    (".##.", "#..#", "#..#", ".###", "...#", "...#", "...#", ".##."),
)
PIXELS = 32
CLASSES = 10
ON, OFF = 90, -30
_MASK = (1 << 64) - 1


class Lcg:
    """64-bit linear congruential generator (Knuth MMIX constants)."""

    def __init__(self, seed: int) -> None:
        self.state = (seed * 2862933555777941757 + 3037000493) & _MASK

    def below(self, bound: int) -> int:
        self.state = (self.state * 6364136223846793005 + 1442695040888963407) & _MASK
        return (self.state >> 33) % bound


def template(digit: int) -> tuple[int, ...]:
    return tuple(ON if pixel == "#" else OFF for row in GLYPHS[digit] for pixel in row)


def make_dataset(
    count: int, seed: int, noise: int = 35, flip_per_mille: int = 80
) -> tuple[list[tuple[int, ...]], list[int]]:
    """Return ``count`` (rows, labels): uniform noise and random pixel flips."""
    if not 1 <= count <= 100_000:
        raise ValueError("count out of range")
    rng = Lcg(seed)
    rows: list[tuple[int, ...]] = []
    labels: list[int] = []
    for _ in range(count):
        digit = rng.below(CLASSES)
        pixels = []
        for value in template(digit):
            if rng.below(1000) < flip_per_mille:
                value = OFF if value == ON else ON
            value += rng.below(2 * noise + 1) - noise
            pixels.append(max(-127, min(127, value)))
        rows.append(tuple(pixels))
        labels.append(digit)
    return rows, labels
