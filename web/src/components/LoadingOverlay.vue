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
