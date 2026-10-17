/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue';
  const component: DefineComponent<object, object, unknown>;
  export default component;
}

// `@office-rs/font-runtime/vite` 的 virtual module（共享包硬编码的公开契约
// id，spec 2026-10-17 全局约束）。
declare module 'virtual:rword-fonts' {
  export const manifestUrl: string;
}
