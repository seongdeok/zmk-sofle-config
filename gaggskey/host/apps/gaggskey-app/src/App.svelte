<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import type { Snapshot, UiCommand } from "./types";

  let snap = $state<Snapshot | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);

  onMount(() => {
    invoke<Snapshot>("get_snapshot").then((s) => (snap = s));
    const un = listen<Snapshot>("snapshot", (e) => (snap = e.payload));
    return () => un.then((f) => f());
  });

  async function send(c: UiCommand, confirmText?: string) {
    if (confirmText && !confirm(confirmText)) return;
    busy = true;
    error = null;
    try {
      await invoke("send_command", { command: c });
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  const st = $derived(snap?.state ?? null);
  const hello = $derived(snap?.hello ?? null);
  const names = $derived(
    Array.from({ length: hello?.layers ?? 0 }, (_, i) => snap?.layer_names[i] || `Layer ${i}`),
  );
  const pct = (v: number | null | undefined) => (v == null ? "--" : `${v}%`);
  const level = (v: number | null | undefined) =>
    v == null ? "unknown" : v <= 15 ? "low" : v <= 40 ? "mid" : "ok";
</script>

<main>
  <header>
    <div class="title">
      <h1>{hello?.name ?? "GaggsKey"}</h1>
      {#if snap?.connected}
        <span class="badge ok">{snap.via?.startsWith("usb") ? "USB" : "BLE"} 연결됨</span>
      {:else}
        <span class="badge off">연결 안 됨</span>
      {/if}
    </div>
    {#if st}
      <div class="chips">
        {#if st.caps_lock}<span class="chip on">CAPS</span>{/if}
        <span class="chip">{st.wpm} WPM</span>
        <span class="chip">{st.activity}</span>
      </div>
    {/if}
  </header>

  {#if !snap?.connected || !st}
    <section class="empty">
      <p>키보드를 찾는 중…</p>
      {#if snap?.error}<p class="muted small">{snap.error}</p>{/if}
      <p class="muted small">USB로 연결하거나, BLE로 페어링된 상태인지 확인하세요.</p>
    </section>
  {:else}
    <section>
      <h2>배터리</h2>
      <div class="batteries">
        {#each [["왼쪽", st.battery.left, true], ["오른쪽", st.battery.right, st.periph_connected]] as [label, v, online]}
          <div class="battery {level(v as number | null)}" class:offline={!online}>
            <div class="bar"><div class="fill" style="width:{v ?? 0}%"></div></div>
            <span class="label">{label}</span>
            <span class="value">{online ? pct(v as number | null) : "오프라인"}</span>
          </div>
        {/each}
      </div>
      {#if st.usb_powered}<p class="muted small">USB 전원 공급 중</p>{/if}
    </section>

    <section>
      <h2>레이어</h2>
      <div class="layers">
        {#each names as name, i}
          <button
            class="layer"
            class:active={st.layer === i}
            class:on={st.layers_active.includes(i) && st.layer !== i}
            disabled={busy}
            onclick={() => send({ cmd: "layer_to", arg: i })}
            oncontextmenu={(e) => { e.preventDefault(); send({ cmd: "layer_toggle", arg: i }); }}
            title="클릭: 전환 · 우클릭: 토글"
          >
            <span class="idx">{i}</span>{name}
          </button>
        {/each}
      </div>
    </section>

    <section>
      <h2>출력</h2>
      <div class="segmented">
        {#each ["usb", "ble"] as const as o}
          <button class:active={st.output === o} disabled={busy} onclick={() => send({ cmd: "out_select", arg: o })}>
            {o.toUpperCase()}
          </button>
        {/each}
      </div>
    </section>

    <section>
      <h2>블루투스 프로필</h2>
      <div class="profiles">
        {#each Array.from({ length: hello?.ble_profiles ?? 0 }) as _, i}
          <button class="profile" class:active={st.ble_profile === i} disabled={busy}
                  onclick={() => send({ cmd: "bt_select", arg: i })}>
            <span class="dot" class:connected={st.ble_connected.includes(i)}></span>
            {i + 1}
          </button>
        {/each}
      </div>
      {#if st.ble_profile_open}<p class="muted small">현재 프로필은 페어링 대기 중</p>{/if}
    </section>

    <details>
      <summary>고급</summary>
      <div class="danger">
        <button disabled={busy} onclick={() => send({ cmd: "bt_clear" }, `프로필 ${st!.ble_profile + 1}의 페어링을 지울까요?`)}>현재 BT 프로필 페어링 해제</button>
        <button disabled={busy} onclick={() => send({ cmd: "reset" }, "키보드를 재부팅할까요?")}>재부팅</button>
        <button disabled={busy || !hello?.caps.includes("dangerous")} onclick={() => send({ cmd: "bootloader" }, "부트로더(UF2)로 재부팅할까요?")}>부트로더 진입</button>
      </div>
      <p class="muted small">{snap.via} · proto v{hello?.proto}</p>
    </details>
  {/if}

  {#if error}<div class="toast" role="alert">{error}</div>{/if}
</main>
