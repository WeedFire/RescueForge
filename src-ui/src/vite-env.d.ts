/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_APP_TITLE: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}

// 编译期注入的应用版本（见 vite.config.ts 的 define，单一来源 = package.json version）
declare const __APP_VERSION__: string
