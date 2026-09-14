# DevicesPage 全量刷新 + 全屏菊花遮罩 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix `DevicesPage` "刷新状态" button so it (a) actually refreshes everything — device-list metadata (`bound` / `device-name` / `pctype`) **plus** runtime status — and (b) shows a full-screen loading overlay with cancel during the fetch.

**Architecture:** Two independent flows in `DevicesPage`: `silentFirstLoad()` runs on mount (no overlay, reuses cached `auth.me`); `refreshAll()` runs on user button click (overlay + abort + progress). A new global `<LoadingOverlay>` component wraps both the cancel button and progress display. Backend BFF is unchanged — the SPA calls the existing `/api/me` and `/api/device-status/:port` endpoints with a shared `AbortController`.

**Tech Stack:** Vue 3 (Composition API + `<script setup>`) · TypeScript · Pinia · vue-tsc (type-check bundled into `npm run build`) · no test runner (manual browser verification)

**Spec:** [`../specs/2026-09-14-devices-refresh-loading-design.md`](../specs/2026-09-14-devices-refresh-loading-design.md)

---

## File Structure

| File                                                  | Responsibility                                                                 |
| ----------------------------------------------------- | ------------------------------------------------------------------------------ |
| `web/src/components/LoadingOverlay.vue` (new)          | Global reusable modal-overlay component (Teleport to body, scoped styles)       |
| `web/src/api.ts` (modify)                              | Add optional 2nd `RequestInit` param to `apiGet` so callers can pass `signal`   |
| `web/src/store.ts` (modify)                            | `auth.probe()` accepts optional `AbortSignal`                                   |
| `web/src/pages/DevicesPage.vue` (modify)               | Wire `silentFirstLoad` (onMounted) + `refreshAll` (button) + `<LoadingOverlay>` |

These four are decoupled enough to land in order. No file does two jobs.

---

## Task 1: Create `LoadingOverlay.vue` (pure UI, no deps)

**Files:**
- Create: `web/src/components/LoadingOverlay.vue`

- [ ] **Step 1: Create the file with the full template + script + style from spec §4.1**

`web/src/components/LoadingOverlay.vue`:

```vue
<script setup lang="ts">
defineProps<{
  visible: boolean
  title?: string
  progress?: { done: number; total: number }
  cancelable?: boolean
}>()
defineEmits<{ (e: 'cancel'): void }>()
</script>

<template>
  <Teleport to="body">
    <Transition name="overlay-fade">
      <div v-if="visible" class="overlay-root">
        <div class="overlay-mask" />
        <div class="overlay-card" role="dialog" aria-modal="true" aria-live="polite">
          <div class="spinner" aria-hidden="true" />
          <p class="overlay-title">{{ title ?? '加载中…' }}</p>
          <p v-if="progress" class="overlay-progress">({{ progress.done }}/{{ progress.total }})</p>
          <button
            v-if="cancelable"
            type="button"
            class="ghost overlay-cancel"
            @click="$emit('cancel')"
          >取消</button>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.overlay-fade-enter-active,
.overlay-fade-leave-active {
  transition: opacity 0.15s ease;
}
.overlay-fade-enter-from,
.overlay-fade-leave-to {
  opacity: 0;
}

.overlay-root { /* container for Transition; lets fade apply to mask + card together */ }

.overlay-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  z-index: 999;
}

.overlay-card {
  position: fixed;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 24px 32px;
  z-index: 1000;
  min-width: 240px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.2);
}

.overlay-title {
  margin: 0;
  font-size: 1rem;
  color: var(--text);
}

.overlay-progress {
  margin: 0;
  font-size: 0.85rem;
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
}

.spinner {
  width: 32px;
  height: 32px;
  border: 3px solid var(--border);
  border-top-color: var(--primary);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

@media (prefers-reduced-motion: reduce) {
  .spinner { animation-duration: 3s; }
}

.overlay-cancel {
  min-height: 36px;
  padding: 6px 16px;
  margin-top: 4px;
}
</style>
```

- [ ] **Step 2: Verify type-check + build**

Run from `web/`:
```bash
cd web && npm run build
```
Expected: completes with no TS errors and writes `dist/`.

- [ ] **Step 3: Commit**

```bash
git add web/src/components/LoadingOverlay.vue
git commit -m "feat(web): add LoadingOverlay component (global, reusable)"
```

---

## Task 2: Extend `apiGet` with optional `RequestInit` for `AbortSignal`

**Files:**
- Modify: `web/src/api.ts:46-47` (the `apiGet` arrow function only — leave `apiPost`/`apiPut`/`apiDelete` untouched per spec §4.4 YAGNI)

- [ ] **Step 1: Replace `apiGet` to accept an optional 2nd arg**

In `web/src/api.ts`, replace the entire `apiGet` block:

```ts
export const apiGet = <T>(path: string, init?: RequestInit): Promise<T> =>
  fetch(path, { credentials: 'include', ...init }).then((r) => handle<T>(r))
```

The old code was:

```ts
export const apiGet = <T>(path: string): Promise<T> =>
  fetch(path, { credentials: 'include' }).then((r) => handle<T>(r))
```

`{ credentials: 'include', ...init }` — `init` comes second so callers can override `credentials` if ever needed, but in practice they only override `signal`. The `handle` function needs no change (spec §4.4 explains AbortError is rejected by `fetch` itself before `handle` is reached).

- [ ] **Step 2: Verify no other call sites are broken**

The new 2nd arg is optional, so existing call sites compile unchanged. Run:
```bash
cd web && npm run build
```
Expected: zero TS errors. The `build` script runs `vue-tsc --noEmit && vite build`.

- [ ] **Step 3: Commit**

```bash
git add web/src/api.ts
git commit -m "feat(web): apiGet accepts optional RequestInit for AbortSignal passthrough"
```

---

## Task 3: Extend `auth.probe` with optional `AbortSignal`

**Files:**
- Modify: `web/src/store.ts:20-32` (the `probe` function inside `useAuthStore`)

- [ ] **Step 1: Replace `probe` signature + body**

In `web/src/store.ts`, replace the existing `probe` function:

```ts
async function probe(signal?: AbortSignal): Promise<boolean> {
  try {
    me.value = await apiGet<MeResponse>('/api/me', { signal })
    return true
  } catch (e) {
    if (e instanceof UnauthorizedError) {
      me.value = null
    }
    // Network / 5xx: keep previous me (don't flip to logged-out on
    // transient errors), but signal not-logged-in for routing.
    return false
  }
}
```

The old code was:

```ts
async function probe(): Promise<boolean> {
  try {
    me.value = await apiGet<MeResponse>('/api/me')
    return true
  } catch (e) {
    if (e instanceof UnauthorizedError) {
      me.value = null
    }
    return false
  }
}
```

The 6-line comment block immediately above `probe` ("Probe whether the current request…") must be preserved verbatim — it documents a critical bug-fix history.

- [ ] **Step 2: Verify router guard call site still compiles**

The router beforeEach calls `auth.probe()` with no args — still valid because `signal` is optional.

```bash
cd web && npm run build
```
Expected: zero TS errors.

- [ ] **Step 3: Commit**

```bash
git add web/src/store.ts
git commit -m "feat(web): auth.probe accepts optional AbortSignal"
```

---

## Task 4: Rewire `DevicesPage.vue` with `refreshAll` + `silentFirstLoad` + `<LoadingOverlay>`

**Files:**
- Modify: `web/src/pages/DevicesPage.vue` (entire `<script setup>` block + template button + add `<LoadingOverlay>` at end of template)

- [ ] **Step 1: Update the `<script setup>` imports + remove obsolete code**

Replace the top of `<script setup>` (the `import { computed, onMounted, ref } from 'vue'` line plus the imports + `auth = useAuthStore()`, `loading = ref(true)`, `error = ref('')` declarations) with:

```ts
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { apiGet, UnauthorizedError } from '../api'
import type { DeviceStatus, UserDevice } from '../api'
import { useAuthStore } from '../store'
import LoadingOverlay from '../components/LoadingOverlay.vue'

const auth = useAuthStore()
const router = useRouter()

const error = ref('')
```

The `loading = ref(true)` flag and its `loading.value = false` reset are removed (spec §4.2: silentFirstLoad doesn't gate rendering with a spinner).

Then delete these three things entirely (they're replaced by `refreshing`):

- `const probing = ref<Record<number, boolean>>({})`
- `function isProbing(d: UserDevice): boolean { return probing.value[d.port] === true }`
- `const probingAny = computed(() => Object.values(probing.value).some(Boolean))`

Delete these two functions entirely (replaced by `refreshAll` + `silentFirstLoad`):

- `async function probe(d: UserDevice) { ... }`
- `function probeAll() { for (const d of devices.value) probe(d) }`

- [ ] **Step 2: Add `refreshing` state + `refreshAll` + `cancelRefresh` + `silentFirstLoad`**

Immediately after the `error = ref('')` line (and before the `// 用户设备清单…` comment), insert:

```ts
// 全量刷新状态：null = 空闲；非 null = 正在刷新（含菊花进度）
const refreshing = ref<{ done: number; total: number } | null>(null)
let refreshAbort: AbortController | null = null

async function refreshAll() {
  if (refreshing.value !== null) return         // 防重入
  const ctrl = new AbortController()
  refreshAbort = ctrl
  const initialDevices = auth.me?.devices ?? []
  refreshing.value = { done: 0, total: initialDevices.length }

  try {
    // (a) 重拉 /api/me
    // probe() 不抛 UnauthorizedError/network 错误——只抛 AbortError 并返回 false。
    // 401 与网络错误的区分靠 me 是否被清空（probe 内部约定）。
    const ok = await auth.probe(ctrl.signal).catch(() => false)
    if (ctrl.signal.aborted) return             // 阶段 (a) 被取消
    if (!ok) {
      if (auth.me === null) {                   // 401：probe 已清空 me
        refreshing.value = null
        refreshAbort = null
        router.push('/login')                   // 路由守卫会接管后续；这里显式触发
        return
      }
      // 网络/5xx：probe 保留旧 me（按 store.ts 的「不轻易登出」约定）
      refreshing.value = null
      refreshAbort = null
      showToast('刷新设备清单失败，请稍后重试', 'warn')
      return
    }

    // (b) 并发刷 device-status（批 ≤ 6）
    const devices = auth.me?.devices ?? []
    const BATCH = 6
    for (let i = 0; i < devices.length; i += BATCH) {
      if (ctrl.signal.aborted) break
      const batch = devices.slice(i, i + BATCH)
      await Promise.allSettled(
        batch.map((d) =>
          apiGet<DeviceStatus>(`/api/device-status/${d.port}`, { signal: ctrl.signal })
            .then((r) => {
              // Race guard: fetch 可能在 abort 后才 resolve（极少数情况），
              // 此时已不算「本次刷新」的结果，不写入 statuses。
              if (ctrl.signal.aborted) return
              statuses.value = { ...statuses.value, [d.port]: r }
            })
            .catch((e) => {
              // 批次中若出现 401，由页面级路由守卫在下次导航时兜底跳 /login。
              if (ctrl.signal.aborted) return
              statuses.value = {
                ...statuses.value,
                [d.port]: { online: false, reason: e instanceof Error ? e.message : String(e) },
              }
            })
            .finally(() => {
              if (refreshing.value) {
                refreshing.value = { ...refreshing.value, done: refreshing.value.done + 1 }
              }
            }),
        ),
      )
    }
  } finally {
    const wasCancelled = ctrl.signal.aborted
    refreshing.value = null
    refreshAbort = null
    if (wasCancelled) showToast('已取消刷新', 'muted')
  }
}

function cancelRefresh() {
  refreshAbort?.abort()
}

async function silentFirstLoad() {
  // 1) 拿到最新设备清单（无 signal、不弹菊花）。401 由路由守卫处理。
  await auth.probe().catch(() => {})
  const devices = auth.me?.devices ?? []
  // 2) fire-and-forget 触发每台设备状态探测。不 await、不写 refreshing。
  for (const d of devices) {
    apiGet<DeviceStatus>(`/api/device-status/${d.port}`)
      .then((r) => {
        statuses.value = { ...statuses.value, [d.port]: r }
      })
      .catch((e) => {
        if (e instanceof UnauthorizedError) return
        statuses.value = {
          ...statuses.value,
          [d.port]: { online: false, reason: e instanceof Error ? e.message : String(e) },
        }
      })
  }
}
```

- [ ] **Step 3: Replace the existing `onMounted` block**

Replace:

```ts
onMounted(async () => {
  if (!auth.me) await auth.probe()
  loading.value = false
  probeAll()
})
```

with:

```ts
onMounted(() => {
  silentFirstLoad()
})
```

- [ ] **Step 4: Update the button binding + add `<LoadingOverlay>` to the template**

In the template's `<button class="ghost refresh-btn" …>` element, replace:

```html
<button class="ghost refresh-btn" :disabled="probingAny" @click="probeAll">
  {{ probingAny ? '探测中…' : '刷新状态' }}
</button>
```

with:

```html
<button
  class="ghost refresh-btn"
  :disabled="refreshing !== null"
  @click="refreshAll"
>
  {{ refreshing !== null ? '刷新中…' : '刷新状态' }}
</button>
```

Immediately before the closing `</div>` of the outermost template wrapper (i.e. just before the toast `Transition`), add:

```html
<LoadingOverlay
  :visible="refreshing !== null"
  :progress="refreshing ?? undefined"
  title="正在刷新设备状态…"
  cancelable
  @cancel="cancelRefresh"
/>
```

- [ ] **Step 5: Verify build passes (type-check + vite)**

```bash
cd web && npm run build
```
Expected: zero TS errors, `dist/` written. If you see `Cannot find name 'LoadingOverlay'` or `probingAny is declared but never used`, recheck Steps 1-4.

- [ ] **Step 6: Commit**

```bash
git add web/src/pages/DevicesPage.vue
git commit -m "feat(web): DevicesPage full refresh + LoadingOverlay + cancel"
```

---

## Task 5: Manual browser verification (acceptance checklist)

**No files modified.** Run the BFF locally + serve the SPA, then exercise the page in a real browser and tick each item from spec §6.1.

- [ ] **Step 1: Start the BFF in a shell**

From repo root:
```bash
cargo run
```
Expected: server binds 127.0.0.1:8100 with `web_static_dir=./web/dist` (default).

- [ ] **Step 2: Build the SPA in a second shell**

```bash
cd web && npm run build
```
Expected: `dist/index.html` and `dist/assets/index-*.js` written, the latter with content that includes the strings `LoadingOverlay` / `Loading…` / `正在刷新设备状态` (sanity-check that the new bundle shipped).

- [ ] **Step 3: Functional acceptance — run through spec §6.1 item by item**

Open `http://127.0.0.1:8100/`, log in with a valid key, land on `/devices`. For each row below, check the box only after you observe the described behaviour:

- [ ] 6.1.a Click "刷新状态" → overlay appears immediately, button disabled
- [ ] 6.1.b Overlay card shows "正在刷新设备状态…" plus live "(n/N)" counter
- [ ] 6.1.c Overlay card has a "取消" button
- [ ] 6.1.d Click "取消" → overlay closes immediately, **device cards retain previous state** (no flicker to new values), toast "已取消刷新" appears and self-dismisses in ~3.5s
- [ ] 6.1.e Let a refresh finish → overlay closes, cards show fresh `bound` / `device-name` / runtime status
- [ ] 6.1.f (Cross-session) In admin panel, change a device's `bound` to true; back on user `/devices`, click "刷新状态"; that device's badge updates to "已绑定"
- [ ] 6.1.g (Fault-inject) Temporarily edit `web/src/api.ts` to throw before `fetch`, click "刷新状态" → overlay closes, toast "刷新设备清单失败，请稍后重试"; revert the edit

If any item fails, stop and file a follow-up — do not paper over with a smaller fix.

- [ ] **Step 4: First-load no-overlay check**

Hard-reload `/devices`. Confirm **no overlay flashes** on first load (silentFirstLoad path). Device cards populate silently as responses arrive.

- [ ] **Step 5: prefers-reduced-motion check**

In Chrome DevTools → Rendering → "Emulate CSS media feature prefers-reduced-motion: reduce", reload, click 刷新状态. Spinner should rotate at ~3s/rev instead of 0.8s/rev.

- [ ] **Step 6: No leftover code**

```bash
grep -nE 'probingAny|function probeAll|function probe\(|loading\.value' web/src/pages/DevicesPage.vue
```
Expected: no output. (All obsolete symbols were removed in Task 4 Step 1.)

- [ ] **Step 7: Final commit if any fix-ups were needed**

```bash
git add -A
git commit -m "fix(web): address manual acceptance findings"
```
(Skip this commit if nothing changed.)

---

## Notes for the executor

- **Do NOT add tests** for this SPA — there's no test runner configured and adding one (vitest + jsdom + @vue/test-utils) would balloon the diff beyond the spec's scope. Build-time `vue-tsc` is the automated gate.
- **Do NOT modify `web/src/style.css`** — the overlay's styles are scoped inside the component.
- **Do NOT touch `src/`** — backend is out of scope.
- **No new dependencies** — `LoadingOverlay` uses only built-ins (`Teleport`, `Transition`) already shipped with Vue 3.
- If `npm run build` fails on `vue-tsc`, read the error carefully: most likely causes are (a) a typo in the `refreshing` / `refreshAll` block, or (b) forgetting to delete one of `loading.value`, `probeAll`, `probingAny`, `probe`. Re-grep Step 6 of Task 5 if needed.
