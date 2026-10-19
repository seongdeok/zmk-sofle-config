<script lang="ts">
  import type { Keymap, KeyLabel } from "./types";

  let {
    keymap,
    names,
    current,
    active,
  }: { keymap: Keymap; names: string[]; current: number; active: number[] } = $props();

  // null = follow the keyboard's current layer
  let pinned = $state<number | null>(null);
  const shown = $derived(pinned ?? current);

  const PAD = 6;
  const bounds = $derived.by(() => {
    let w = 0, h = 0;
    for (const k of keymap.keys) {
      w = Math.max(w, k.x + k.w);
      h = Math.max(h, k.y + k.h);
    }
    return { w: w + PAD * 2, h: h + PAD * 2 + 20 };
  });

  // Resolve transparent keys to what actually fires (next lower active layer, then default).
  function resolve(layer: number, pos: number): { k: KeyLabel; from: number } | null {
    const k = keymap.layers[layer]?.[pos];
    if (!k) return null;
    if (k.kind !== "trans") return { k, from: layer };
    for (let l = layer - 1; l >= 0; l--) {
      if (l !== 0 && !active.includes(l)) continue;
      const below = keymap.layers[l]?.[pos];
      if (below && below.kind !== "trans") return { k: below, from: l };
    }
    return { k, from: layer };
  }

  const fit = (s: string, w: number) => {
    const max = Math.max(3, Math.floor(w / 13));
    return s.length > max ? s.slice(0, max - 1) + "…" : s;
  };
</script>

<div class="tabs">
  <button class:active={pinned === null} onclick={() => (pinned = null)} title="키보드의 현재 레이어를 따라감">
    자동
  </button>
  {#each names as n, i}
    <button
      class:active={pinned === i}
      class:live={current === i}
      onclick={() => (pinned = pinned === i ? null : i)}
    >{n}</button>
  {/each}
</div>

<svg viewBox="0 0 {bounds.w} {bounds.h}" class="kb" role="img" aria-label="keymap {names[shown]}">
  <g transform="translate({PAD} {PAD})">
    {#each keymap.keys as g, pos}
      {@const r = resolve(shown, pos)}
      {#if r}
        <g
          transform={g.r ? `rotate(${g.r / 100} ${g.rx} ${g.ry})` : undefined}
          class="key {r.k.kind}"
          class:inherited={r.from !== shown}
        >
          <title>{pos}: {r.k.raw}{r.from !== shown ? ` (← ${names[r.from]})` : ""}</title>
          <rect x={g.x + 4} y={g.y + 4} width={g.w - 8} height={g.h - 8} rx="12" />
          <text
            x={g.x + g.w / 2}
            y={g.y + g.h / 2 + (r.k.hold ? -4 : 6)}
            class="tap"
            class:long={r.k.tap.length > 4}
          >{fit(r.k.tap, g.w)}</text>
          {#if r.k.hold}
            <text x={g.x + g.w / 2} y={g.y + g.h - 16} class="hold">{fit(r.k.hold, g.w)}</text>
          {/if}
        </g>
      {/if}
    {/each}
  </g>
</svg>
<p class="muted small legend">
  {pinned === null ? "현재 레이어 표시 중" : `${names[shown]} 미리보기`} · 흐린 키는 아래 레이어에서 내려온 값(▽) · 키에 마우스를 올리면 원본 바인딩
</p>

<style>
  .tabs { display: flex; flex-wrap: wrap; gap: 4px; margin-bottom: 10px; }
  .tabs button { padding: 4px 10px; font-size: 12px; }
  .tabs button.live:not(.active) { border-color: var(--accent); color: var(--accent); }
  .kb { width: 100%; height: auto; display: block; }
  .key rect { fill: var(--bg); stroke: var(--border); stroke-width: 2; }
  .key text { text-anchor: middle; fill: var(--fg); font-family: inherit; }
  .tap { font-size: 30px; font-weight: 600; }
  .tap.long { font-size: 22px; }
  .hold { font-size: 17px; fill: var(--muted) !important; }
  .key.layer rect { stroke: var(--accent); }
  .key.layer .hold { fill: var(--accent) !important; }
  .key.mod rect { stroke: #6aa8ff; }
  .key.mod .hold { fill: #6aa8ff !important; }
  .key.bt rect, .key.out rect { stroke: #9b7bff; }
  .key.mouse rect { stroke: #3ccf91; }
  .key.media rect { stroke: #e98bd1; }
  .key.system rect { stroke: var(--low); }
  .key.none rect { opacity: .35; }
  .key.trans text { fill: var(--muted); }
  .key.inherited { opacity: .4; }
  .legend { margin-top: 6px; }
</style>
