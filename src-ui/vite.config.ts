import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { fileURLToPath, URL } from "node:url";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const host = process.env.TAURI_DEV_HOST;

// 单一版本来源：package.json 的 version。
// tauri.conf.json 已用 "../src-ui/package.json" 引用同一版本，
// 这里把版本编译期注入 __APP_VERSION__，供前端 dev 环境回退显示（运行时优先 getVersion()）。
const pkg = JSON.parse(readFileSync(resolve(__dirname, "package.json"), "utf-8")) as { version: string };

export default defineConfig({
  plugins: [
    vue(),
    {
      // Tauri 内嵌前端使用 tauri://localhost（或 https://tauri.localhost）自定义协议，
      // 该协议响应不带 CORS 头。Vite 默认给 <script>/<link> 加 crossorigin，
      // 浏览器会以 CORS 模式校验并拦截 CSS/JS —— 表现为打包后样式全丢、菜单不可见
      // （dev 模式用 <style> 注入所以正常）。构建时统一剥掉该属性。
      name: "tauri-strip-crossorigin",
      transformIndexHtml(html) {
        return html.replace(/\s+crossorigin(?:\s*=\s*"[^"]*")?/g, "");
      },
    },
  ],

  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },

  // 打包必须用相对路径：Tauri 内嵌前端走 tauri://localhost（或 https://tauri.localhost）
  // 自定义协议，绝对路径 /assets/ 在该协议下解析不稳定。
  base: "./",

  build: {
    // 关闭 modulePreload：其生成的 <link rel="modulepreload"> 同样带 crossorigin。
    modulePreload: false,
    // 产物内联阈值调低，减少散碎 chunk
    assetsInlineLimit: 4096,
  },

  // Prevent Vite from pre-bundling @tauri-apps/api (it needs native Tauri IPC)
  optimizeDeps: {
    exclude: ["@tauri-apps/api"],
  },

  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },

  clearScreen: false,

  server: {
    port: 1420,
    strictPort: true,
    host: host || "0.0.0.0",
    allowedHosts: true,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
