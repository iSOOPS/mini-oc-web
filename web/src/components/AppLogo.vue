<script setup lang="ts">
/**
 * mini-oc-web brand logo (MOW monogram): dark rounded square + white
 * geometric MOW. Sized via the `size` prop (square).
 *
 * The letters are hand-drawn paths (no <text>, no font dependency), so the
 * mark renders pixel-identically everywhere — page, favicon, offline
 * intranet deployments — with exact centering (cap band y:42..86 and
 * x band 9..119 are both symmetric around 64).
 */
withDefaults(defineProps<{ size?: number }>(), { size: 64 })
</script>

<template>
  <svg
    class="app-logo"
    :width="size"
    :height="size"
    viewBox="0 0 128 128"
    xmlns="http://www.w3.org/2000/svg"
    role="img"
    aria-label="mini-oc-web logo"
  >
    <rect x="0" y="0" width="128" height="128" rx="12" ry="12" fill="#1f2937" />
    <path
      fill="#ffffff"
      fill-rule="evenodd"
      d="M9,42 L17,42 L25,66 L33,42 L41,42 L41,86 L33,86 L33,58 L27,72 L23,72 L17,58 L17,86 L9,86 Z
        M58,42 H70 A12,12 0 0 1 82,54 V74 A12,12 0 0 1 70,86 H58 A12,12 0 0 1 46,74 V54 A12,12 0 0 1 58,42 Z
        M59,50 H69 A5,5 0 0 1 74,55 V73 A5,5 0 0 1 69,78 H59 A5,5 0 0 1 54,73 V55 A5,5 0 0 1 59,50 Z
        M87,86 L95,86 L103,62 L111,86 L119,86 L119,42 L111,42 L111,70 L105,56 L101,56 L95,70 L95,42 L87,42 Z"
    />
  </svg>
</template>

<style scoped>
.app-logo {
  display: block;
  /* Keep the rounded corners crisp at small sizes (favicon-grade AA). */
  shape-rendering: geometricPrecision;
}
</style>
