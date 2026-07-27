import { test } from 'node:test';
import assert from 'node:assert/strict';

import { isotonic, placeColumn, type LabelBox } from '../src/labels.ts';

/** Deterministic PRNG, so a failure can be reproduced from the seed alone. */
function rng(seed: number) {
  let s = seed >>> 0;
  return () => {
    s ^= s << 13;
    s >>>= 0;
    s ^= s >> 17;
    s ^= s << 5;
    s >>>= 0;
    return s / 0xffffffff;
  };
}

function cost(y: number[], boxes: LabelBox[]): number {
  return y.reduce(
    (acc, v, i) => acc + (boxes[i].weight ?? 1) * (v - boxes[i].ideal) ** 2,
    0,
  );
}

test('isotonic leaves an already-increasing sequence alone', () => {
  const t = [1, 2, 3, 10, 20];
  assert.deepEqual(isotonic(t, t.map(() => 1)), t);
});

test('isotonic averages a violating pair', () => {
  // 5 then 1 must become 3, 3 — the least-squares non-decreasing fit.
  assert.deepEqual(isotonic([5, 1], [1, 1]), [3, 3]);
});

test('isotonic respects weights', () => {
  // The heavier point drags the shared value toward itself.
  const out = isotonic([5, 1], [3, 1]);
  assert.equal(out[0], out[1]);
  assert.equal(out[0], (3 * 5 + 1 * 1) / 4);
});

test('isotonic output is always non-decreasing', () => {
  const r = rng(12345);
  for (let trial = 0; trial < 500; trial++) {
    const n = 1 + Math.floor(r() * 30);
    const t = Array.from({ length: n }, () => r() * 100);
    const w = Array.from({ length: n }, () => 0.1 + r() * 3);
    const out = isotonic(t, w);
    for (let i = 1; i < out.length; i++) {
      assert.ok(out[i] >= out[i - 1] - 1e-9, `not monotone at ${i}`);
    }
    // And the fit preserves the total weighted mass, which is the property
    // that makes it a projection rather than an arbitrary smoothing.
    const massIn = t.reduce((s, v, i) => s + w[i] * v, 0);
    const massOut = out.reduce((s, v, i) => s + w[i] * v, 0);
    assert.ok(Math.abs(massIn - massOut) < 1e-6);
  }
});

test('labels that already fit are not moved at all', () => {
  const boxes: LabelBox[] = [
    { ideal: 20, height: 10 },
    { ideal: 60, height: 10 },
    { ideal: 100, height: 10 },
  ];
  const { positions, dropped } = placeColumn(boxes, 0, 200);
  assert.deepEqual(dropped, []);
  assert.deepEqual(positions, [20, 60, 100]);
});

test('colliding labels are separated by exactly their height', () => {
  const boxes: LabelBox[] = [
    { ideal: 50, height: 10 },
    { ideal: 52, height: 10 },
    { ideal: 54, height: 10 },
  ];
  const { positions } = placeColumn(boxes, 0, 200);
  assert.ok(positions[1] - positions[0] >= 10 - 1e-9);
  assert.ok(positions[2] - positions[1] >= 10 - 1e-9);
  // Symmetric input, symmetric answer: the middle label should not have moved.
  assert.ok(Math.abs(positions[1] - 52) < 1e-9);
});

test('order is preserved — a label never overtakes its neighbour', () => {
  const r = rng(999);
  for (let trial = 0; trial < 300; trial++) {
    const n = 2 + Math.floor(r() * 25);
    const boxes: LabelBox[] = Array.from({ length: n }, () => ({
      ideal: r() * 300,
      height: 8 + r() * 6,
      weight: 0.2 + r() * 2,
    }));
    const { positions, dropped } = placeColumn(boxes, 0, 600);
    assert.deepEqual(dropped, []);
    const order = boxes
      .map((b, i) => i)
      .sort((a, b) => boxes[a].ideal - boxes[b].ideal);
    for (let i = 1; i < order.length; i++) {
      const prev = positions[order[i - 1]];
      const cur = positions[order[i]];
      const gap = (boxes[order[i - 1]].height + boxes[order[i]].height) / 2;
      assert.ok(
        cur - prev >= gap - 1e-6,
        `trial ${trial}: labels ${i - 1}/${i} overlap (${cur - prev} < ${gap})`,
      );
    }
  }
});

test('the placement is optimal, not merely feasible', () => {
  // The claim in the module docstring is that this minimises weighted squared
  // displacement. Check it against random feasible alternatives: none may beat
  // it. A relaxation-based placer fails this.
  const r = rng(4242);
  for (let trial = 0; trial < 200; trial++) {
    const n = 2 + Math.floor(r() * 6);
    const h = 10;
    const boxes: LabelBox[] = Array.from({ length: n }, () => ({
      ideal: 40 + r() * 40,
      height: h,
      weight: 0.5 + r() * 2,
    })).sort((a, b) => a.ideal - b.ideal);

    const { positions } = placeColumn(boxes, 0, 400);
    const best = cost(positions, boxes);

    for (let k = 0; k < 400; k++) {
      // A random feasible arrangement: pick a start and stack with gaps.
      const start = r() * 120;
      const cand: number[] = [];
      let y = start;
      for (let i = 0; i < n; i++) {
        y += i === 0 ? 0 : h + r() * 5;
        cand.push(y);
      }
      if (cand[0] < h / 2 || cand[n - 1] > 400 - h / 2) continue;
      assert.ok(
        cost(cand, boxes) >= best - 1e-6,
        `trial ${trial}: found a cheaper feasible layout (${cost(cand, boxes)} < ${best})`,
      );
    }
  }
});

test('identical labels stack symmetrically about their shared ideal', () => {
  // The assertion that catches a clamp applied to the regression's input.
  //
  // n identical boxes all wanting the same y must come back centred on that y.
  // Clamping the targets first shifted the whole stack — six labels wanting 50
  // came back centred on 53.33 — while the previous optimality test could never
  // notice, because it only compared against randomly generated LOOSER stacks
  // and so could never find the tighter answer. This failed at n=5 and passed
  // at n=3, which is exactly how it survived.
  for (const n of [1, 2, 3, 4, 5, 6, 9, 15]) {
    const h = 12;
    // Centred in the band, so the box constraint never binds and the
    // unconstrained optimum is the answer. (At n=9 an ideal of 50 would put the
    // top label's edge at −4, and pushing the stack down is then correct.)
    const ideal = 200;
    const boxes: LabelBox[] = Array.from({ length: n }, () => ({ ideal, height: h }));
    const { positions, dropped } = placeColumn(boxes, 0, 400);
    assert.deepEqual(dropped, []);
    const mean = positions.reduce((s, v) => s + v, 0) / n;
    assert.ok(
      Math.abs(mean - ideal) < 1e-9,
      `n=${n}: stack centred on ${mean}, not ${ideal}`,
    );
    // ...and packed as tightly as the constraint allows.
    const sorted = [...positions].sort((a, b) => a - b);
    for (let i = 1; i < n; i++) {
      assert.ok(Math.abs(sorted[i] - sorted[i - 1] - h) < 1e-9);
    }
  }
});

test('a band that forces clamping still yields the constrained optimum', () => {
  // Push the ideal outside the band so the box constraint actually binds.
  const h = 10;
  const n = 5;
  const boxes: LabelBox[] = Array.from({ length: n }, () => ({ ideal: -500, height: h }));
  const { positions } = placeColumn(boxes, 0, 200);
  const sorted = [...positions].sort((a, b) => a - b);
  // Every label wants to be as high as possible, so the stack sits flush
  // against the top of the band.
  assert.ok(Math.abs(sorted[0] - h / 2) < 1e-9, `top label at ${sorted[0]}`);
  for (let i = 1; i < n; i++) {
    assert.ok(Math.abs(sorted[i] - sorted[i - 1] - h) < 1e-9);
  }
});

test('placement is deterministic', () => {
  const boxes: LabelBox[] = Array.from({ length: 40 }, (_, i) => ({
    ideal: (i * 7919) % 300,
    height: 12,
    weight: 1 + (i % 5),
  }));
  const first = placeColumn(boxes, 0, 400);
  for (let i = 0; i < 20; i++) {
    assert.deepEqual(placeColumn(boxes, 0, 400), first);
  }
});

test('when labels cannot fit, the lightest are dropped and reported', () => {
  // Ten 12px labels need 120px; only 60px is available.
  const boxes: LabelBox[] = Array.from({ length: 10 }, (_, i) => ({
    ideal: 30,
    height: 12,
    weight: i + 1,
  }));
  const { positions, dropped } = placeColumn(boxes, 0, 60);
  assert.equal(dropped.length, 5);
  // The five lightest went.
  assert.deepEqual([...dropped].sort((a, b) => a - b), [0, 1, 2, 3, 4]);
  for (const d of dropped) assert.ok(Number.isNaN(positions[d]));
  const kept = positions.filter((p) => Number.isFinite(p));
  assert.equal(kept.length, 5);
});

test('placed labels stay inside the allotted band', () => {
  const r = rng(777);
  for (let trial = 0; trial < 300; trial++) {
    const n = 1 + Math.floor(r() * 20);
    const h = 12;
    const lo = 0;
    const hi = 200;
    const boxes: LabelBox[] = Array.from({ length: n }, () => ({
      // Deliberately pushed far outside the band.
      ideal: -300 + r() * 900,
      height: h,
    }));
    const { positions, dropped } = placeColumn(boxes, lo, hi);
    positions.forEach((y, i) => {
      if (dropped.includes(i)) return;
      assert.ok(y - h / 2 >= lo - 1e-6, `trial ${trial}: ${y} above the band`);
      assert.ok(y + h / 2 <= hi + 1e-6, `trial ${trial}: ${y} below the band`);
    });
  }
});

test('an empty column is not an error', () => {
  assert.deepEqual(placeColumn([], 0, 100), { positions: [], dropped: [] });
});

test('one label sits exactly where it wants to', () => {
  const { positions } = placeColumn([{ ideal: 44, height: 10 }], 0, 200);
  assert.deepEqual(positions, [44]);
});
