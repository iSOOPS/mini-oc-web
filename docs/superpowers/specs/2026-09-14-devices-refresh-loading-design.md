# DevicesPage 全量刷新 + 全屏菊花遮罩

**Date:** 2026-09-14
**Status:** Approved (with amendment) — ADR 19 added after Task 4 code review
**Scope:** 仅前端 SPA；后端 BFF 不动
**Out of Scope:** 后端批量接口、SSE 进度推送、ProjectPage / SessionsPage 的同类改造

> 修复 `DevicesPage`「刷新状态」按钮的两个体验问题：
> (1) 只刷新实时运行态（device-status），**不重拉设备清单元数据**（bound / device-name / pctype），导致绑定状态永远是 onMounted 时的旧快照；
> (2) 「探测中」反馈仅在按钮文字上，无遮罩、无菊花、无取消入口。

---

## 1. 背景与现状

### 1.1 当前实现（`web/src/pages/DevicesPage.vue`）

```ts
async function probe(d: UserDevice) {
  probing.value = { ...probing.value, [d.port]: true }
  try {
    statuses.value = {
      ...statuses.value,
      [d.port]: await apiGet<DeviceStatus>(`/api/device-status/${d.port}`),
    }
  } catch (e) {
    if (e instanceof UnauthorizedError) return
    statuses.value = {
      ...statuses.value,
      [d.port]: { online: false, reason: e instanceof Error ? e.message : String(e) },
    }
  } finally {
    probing.value = { ...probing.value, [d.port]: false }
  }
}

function probeAll() {
  for (const d of devices.value) probe(d)  // fire-and-forget 顺序 fire，无 await
}

onMounted(async () => {
  if (!auth.me) await auth.probe()         // 仅此处拉一次 /api/me
  loading.value = false
  probeAll()
})
```

**问题诊断**：

| #  | 问题                                                                                                          | 影响                                                                              |
| -- | ------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| 1  | `probeAll` 不重拉 `/api/me`，`auth.me.devices[].bound / device-name / pctype` 永远停在 onMounted 时的快照         | 用户在管理后台改了绑定 / 客户端完成新绑定后，前端永远不刷新到新状态                    |
| 2  | 设备状态探测是顺序 fire-and-forget，无并发上限控制，N 大时浏览器连接队列阻塞                                       | 10 台设备最坏 40s（每台 4s 超时）                                                    |
| 3  | 反馈只有按钮文字「探测中…」+ 卡片行3「探测中…」，**没有菊花图标**，**没有遮罩**                                    | 用户误以为页面没在动，可重复点击按钮                                                  |
| 4  | 没有取消入口；某台设备 4s 超时时用户只能等                                                                          | 体验差                                                                            |

### 1.2 相关后端接口（已存在，不动）

- `GET /api/me` → `MeResponse`，含 `devices[]: UserDevice[]`（`bound` / `device-name` / `pctype` 在内）
- `GET /api/device-status/:port` → `DeviceStatus`，4s 单设备超时

---

## 2. 设计决策（来自 brainstorming 对话）

| 决策点              | 选择                                                            | 替代方案                                                       |
| ------------------ | --------------------------------------------------------------- | -------------------------------------------------------------- |
| 刷新范围            | (a) `/api/me` + (b) device-status **都刷**                       | 仅刷 (b)；按住 Shift 才刷 (a)                                   |
| 菊花形态            | **全屏半透遮罩 + 居中卡片（旋转图标 + 进度 + 取消按钮）**         | 页面顶部内联菊花；按钮旁小旋转圈                                  |
| 取消入口            | **菊花卡片下方「取消」按钮**                                      | 不可取消                                                       |
| 进度文案            | **「正在刷新设备状态… (n/N)」实时更新**                          | 固定文案「正在刷新设备状态…」                                    |
| 后端                | **不改**，复用现有 `/api/me` 和 `/api/device-status/:port`         | 加 `POST /api/devices/statuses` 批量接口；SSE 进度推送         |
| 并发策略            | **N ≤ 6 全并发；N > 6 分批 6**                                   | 全部并发；全顺序                                                 |

---

## 3. 行为流程

```
用户点「刷新状态」
        ↓
[a] 校验未在刷新中（disabled = refreshing !== null）
        ↓
[b] 创建 AbortController；refreshing = { done: 0, total: N }
        ↓ 显示菊花（visible=true, cancelable=true）
[c] await auth.probe(signal)
        ├─ 返回 false 且 me===null → 401：refreshAll 显式 router.push('/login'); 菊花关闭; 结束
        ├─ 返回 false 且 me!==null → 网络/5xx：刷新失败 toast; 保留旧 me; 菊花关闭; 结束
        ├─ AbortError → 阶段 (a) 被取消; finally 关闭菊花 + toast「已取消刷新」; 结束
        └─ 返回 true → auth.me 整体替换（devices[].bound/device-name/pctype 全部更新）
        ↓
[d] 切批（每批 ≤ 6），并发触发 GET /api/device-status/:port
        每台返回后：refreshing.done++，写 statuses[port]（不提前关菊花）
        ↓
[e] 全部 settled（或用户点取消）：
        ├─ 用户已点取消 → 不写 statuses，菊花关闭，文案「已取消」（3.5s 后消失）
        └─ 否则 → 菊花关闭
```

### 3.1 取消语义

- 共享 `AbortController.signal` 传给 `/api/me` 和所有 `/api/device-status/:port` fetch
- 点取消 → `controller.abort()` → 所有进行中 fetch 立即 reject（`AbortError`）
- 菊花关闭后**保留旧 statuses.value**（不写部分结果）
- 取消原因不通过 toast 提示；菊花关闭动效已足够反馈

### 3.2 错误语义

| 失败类型                          | 处理                                                                 |
| --------------------------------- | -------------------------------------------------------------------- |
| `/api/me` 返回 401                 | refreshAll 检测 me===null 后显式 router.push('/login')（ADR 19）        |
| `/api/me` 其他错误（5xx/网络）      | 关闭菊花 + toast「刷新设备清单失败，请稍后重试」；保留旧 `auth.me`         |
| 单设备 `/api/device-status` 失败    | 该 port 写 `{online:false, reason: msg}`，**不影响其他设备**，不报错 toast |
| 用户主动取消                       | 不写 statuses，不报错                                                |
| 菊花过程中用户重复点按钮             | 按钮 disabled（refreshing !== null）                                  |

---

## 4. 组件拆分

### 4.1 新建 `web/src/components/LoadingOverlay.vue`（全局通用）

- **定位**：通用模态遮罩组件，不只服务于 DevicesPage
- **Props**
  ```ts
  {
    visible: boolean                      // 控制显隐
    title?: string                        // 默认 '加载中…'
    progress?: { done: number; total: number }  // 可选；提供时显示「(done/total)」
    cancelable?: boolean                  // 默认 false
  }
  ```
- **Emits**：`(e: 'cancel')`
- **DOM 结构**
  ```
  <Teleport to="body">
    <Transition name="overlay-fade">
      <div v-if="visible" class="overlay-root">
        <div class="overlay-mask" />
        <div class="overlay-card" role="dialog" aria-modal="true">
          <div class="spinner" aria-hidden="true" />
          <p class="overlay-title">{{ title }}</p>
          <p v-if="progress" class="overlay-progress">({{ progress.done }}/{{ progress.total }})</p>
          <button v-if="cancelable" class="ghost overlay-cancel" @click="$emit('cancel')">取消</button>
        </div>
      </div>
    </Transition>
  </Teleport>
  ```
- **样式要点**
  - `.overlay-mask`：`position: fixed; inset: 0; background: rgba(0,0,0,0.45); z-index: 999`
  - `.overlay-card`：`position: fixed; top: 50%; left: 50%; transform: translate(-50%, -50%); background: var(--surface); border: 1px solid var(--border); border-radius: 12px; padding: 24px 32px; z-index: 1000; min-width: 240px; display: flex; flex-direction: column; align-items: center; gap: 12px; box-shadow: 0 8px 24px rgba(0,0,0,0.2)`
  - `.spinner`：`width: 32px; height: 32px; border: 3px solid var(--border); border-top-color: var(--primary); border-radius: 50%; animation: spin 0.8s linear infinite`
  - `@keyframes spin { to { transform: rotate(360deg) } }`
  - `@media (prefers-reduced-motion: reduce)`：`animation-duration: 3s`（降速但不消失，符合 WCAG）
  - 取消按钮：`min-height: 36px; padding: 6px 16px; margin-top: 4px`
- **过渡**：`opacity 0→1` (150ms)，无 transform（避免 transform 与居中冲突）

### 4.2 DevicesPage 改造

- 删除 `probing` ref、`probingAny` computed、`probe()` 函数
- 新增 `refreshAll`（用户点按钮时调用，带菊花）：
  ```ts
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
          batch.map(d =>
            apiGet<DeviceStatus>(`/api/device-status/${d.port}`, { signal: ctrl.signal })
              .then(r => {
                // Race guard: fetch 可能在 abort 后才 resolve（极少数情况），
                // 此时已不算「本次刷新」的结果，不写入 statuses。
                if (ctrl.signal.aborted) return
                statuses.value = { ...statuses.value, [d.port]: r }
              })
              .catch(e => {
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
              })
          )
        )
      }
    } finally {
      const wasCancelled = ctrl.signal.aborted
      refreshing.value = null
      refreshAbort = null
      if (wasCancelled) showToast('已取消刷新', 'muted')
    }
  }

  function cancelRefresh() { refreshAbort?.abort() }
  ```
- 新增 `silentFirstLoad`（onMounted 调用，**不带菊花**）：
  ```ts
  async function silentFirstLoad() {
    // 1) 拿到最新设备清单（无 signal、不弹菊花）。401 由路由守卫处理。
    await auth.probe().catch(() => {})
    const devices = auth.me?.devices ?? []
    // 2) fire-and-forget 触发每台设备状态探测。不 await、不写 refreshing。
    //    单设备失败被原 probe() 内部 catch 吞（写离线），无需额外错误处理。
    for (const d of devices) {
      apiGet<DeviceStatus>(`/api/device-status/${d.port}`)
        .then(r => { statuses.value = { ...statuses.value, [d.port]: r } })
        .catch(e => {
          if (e instanceof UnauthorizedError) return
          statuses.value = {
            ...statuses.value,
            [d.port]: { online: false, reason: e instanceof Error ? e.message : String(e) },
          }
        })
    }
  }
  ```
- 按钮：`@click="refreshAll"` `:disabled="refreshing !== null"`，文案随 `refreshing` 切换「刷新状态」/「刷新中…」
- 模板底部加 `<LoadingOverlay :visible="refreshing !== null" :progress="refreshing ?? undefined" title="正在刷新设备状态…" cancelable @cancel="cancelRefresh" />`
- `onMounted` 内 `probeAll()` 调用替换为 `silentFirstLoad()`：**不弹菊花**——首次进入页面的菊花体验差；菊花只服务于用户主动点「刷新状态」的场景。

### 4.3 store.ts 微调

`auth.probe()` 接收可选 `signal`：

```ts
async function probe(signal?: AbortSignal): Promise<boolean> {
  try {
    me.value = await apiGet<MeResponse>('/api/me', { signal })
    return true
  } catch (e) {
    if (e instanceof UnauthorizedError) me.value = null
    return false
  }
}
```

调用点全在 router 守卫和 DevicesPage.refreshAll，**无需其他修改**。

### 4.4 api.ts 微调

**只扩展 `apiGet`**（refreshAll 仅调 GET；YAGNI 不动其他三个）：

```ts
export const apiGet = <T>(path: string, init?: RequestInit): Promise<T> =>
  fetch(path, { credentials: 'include', ...init }).then(r => handle<T>(r))
```

`handle()` 内的 `AbortError` 处理：

```ts
async function handle<T>(r: Response): Promise<T> {
  if (r.status === 401) throw new UnauthorizedError()
  if (!r.ok) {
    const body = await r.json().catch(() => null)
    // ↑ 此处在 fetch 被 abort 后不会再被调用（fetch 本身已 reject AbortError，
    //   不会走到 handle），因此 handle 不需要 abort 特殊处理。
    const msg = body?.error?.message ?? `${r.status} ${r.statusText}`
    if (body?.error?.code) throw new ApiError(body.error.code, msg)
    throw new Error(msg)
  }
  return r.json()
}
```

调用方按 `e instanceof Error && e.name === 'AbortError'` 区分取消 vs 真错误。`refreshAll` 的 `.catch` 分支显式判断后立即 `return`，**不写 `statuses`**，确保取消时不污染状态。

---

## 5. 修改文件清单

| 文件                                       | 类型   | 改动                                                                                              |
| ------------------------------------------ | ------ | ------------------------------------------------------------------------------------------------- |
| `web/src/components/LoadingOverlay.vue`     | 新建   | 全局通用菊花遮罩                                                                                   |
| `web/src/pages/DevicesPage.vue`             | 修改   | 删除 probing 旧逻辑；新增 refreshAll / cancelRefresh / silentFirstLoad；引入 LoadingOverlay；onMounted 改调 silentFirstLoad |
| `web/src/api.ts`                            | 修改   | 仅 apiGet 第二参数 `RequestInit`（用于 signal 透传）                                              |
| `web/src/store.ts`                          | 修改   | `auth.probe(signal?)`                                                                              |
| `web/src/style.css`                         | 不改   | LoadingOverlay scoped 样式足够                                                                     |
| **后端 Rust**                                | **不动** | —                                                                                                  |

---

## 6. 验收标准

### 6.1 功能

- [ ] 用户点「刷新状态」→ 菊花立即出现，按钮 disabled
- [ ] 菊花卡片显示「正在刷新设备状态…」+ 实时进度 `(n/N)`
- [ ] 菊花卡片有「取消」按钮
- [ ] 点取消 → 菊花立即关闭，**页面状态不变**（设备卡片保留旧状态），toast「已取消刷新」
- [ ] 全部完成 → 菊花关闭，设备卡片显示最新状态（含 bound / device-name）
- [ ] 管理后台改了某设备的 `bound` 后，前端点刷新 → 该设备徽章立刻反映「已绑定 / 未绑定」变化
- [ ] `/api/me` 失败（非 401）→ 菊花关闭，toast「刷新设备清单失败，请稍后重试」

### 6.2 兼容与约束

- [ ] N ≤ 6 时所有 device-status 全并发
- [ ] N > 6 时按 6 分批，菊花进度正确反映完成数
- [ ] 菊花过程中重复点按钮不触发第二次刷新（disabled）
- [ ] `prefers-reduced-motion: reduce` 用户菊花降速不消失
- [ ] LoadingOverlay 遮罩覆盖 toast（z-index 999/1000 vs toast 100）
- [ ] SPA `npm run build` 无 TS 错误、无新增 lint 错误

### 6.3 验收命令

```bash
cd web
npm run build       # 期望：无 TS 错误
npm run type-check  # 期望：无 error（如项目有该 script）
```

---

## 7. ADR（本设计增量）

| #   | 决策                                                                      | 理由                                                                                       |
| --- | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| 14  | 不加后端批量接口，前端用 Promise.allSettled + 批 ≤ 6 并发                   | 用户明确选择「保持前端并发，不动后端」；6 是 Chrome 同源连接上限；N 通常远小于此阈值          |
| 15  | LoadingOverlay 用 Teleport to="body"                                      | 避免父组件 z-index / overflow / transform 影响遮罩层级                                       |
| 16  | 取消按钮仅在 cancelable=true 时渲染                                         | 同一组件服务「不可取消的加载」与「可取消的全量刷新」两种场景                                    |
| 17  | onMounted **不弹菊花**，仅 fire-and-forget 触发 `device-status` 探测（复用 store 缓存的 `auth.me`）；菊花只服务于用户主动点「刷新状态」 | 首次进入页面就弹菊花体验差；用户主动刷新才需要菊花是更符合用户预期的边界                       |
| 18  | 取消时菊花关闭后弹一行 toast「已取消刷新」                                  | 取消是用户主动行为，需要显式反馈；菊花关闭动画太短不够醒目 |
| 19  | `refreshAll` 阶段 (a) 用 `auth.probe()` 的 boolean 返回值 + `me===null` 判别 401 / 网络错误；不再依赖 catch UnauthorizedError（probe 内部已吞，catch 永远到不了） | Task 3 设计 probe 吞掉 401/network 只在 AbortError 时抛，导致 §4.2 原参考实现的 catch 是死代码；修复后才能兑现 spec §3 与 §6.1-7 的承诺 |

---

## 8. 不做（明确延后）

| 项目                            | 延后原因                                                            |
| ------------------------------- | ------------------------------------------------------------------- |
| 后端批量 device-status 接口     | 用户明确选择前端并发方案；N 通常 < 6 时前端并发已足够                    |
| SSE 实时进度推送                 | 增加 BFF ↔ SPA 长连接复杂度；前端并发已能把菊花时长控制在秒级             |
| ProjectPage / SessionsPage 同款  | 需求来自 DevicesPage 单独反馈；其他页面暂不涉及「拉多台设备状态」        |
| 菊花主题/暗色模式适配             | 现有 `--surface` / `--border` / `--primary` / `--bg` 变量已覆盖基础主题；新增主题不属本 PR |
