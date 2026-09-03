// rofd Tauri 桌面宿主前端入口。
//
// 复用 web-app 的 App.vue 与全部组件（共享单一 UI 代码），只做两件平台特化：
//   1. 挂载前用 setFileHost 注入 Tauri 原生文件桥（原生打开/保存对话框）；
//   2. antd 组件按 web-app 同样的方式按需注册。
// 字体与 sample.ofd 从 vite publicDir（指向 web-app/public）本地加载，脱离 CDN。

import { createApp } from 'vue';
import { Button, Spin, Tabs, Tooltip } from 'ant-design-vue';
import App from '../../web-app/src/App.vue';
import { setFileHost } from '../../web-app/src/host';
import { tauriFileHost } from './host/tauri';
import 'ant-design-vue/dist/reset.css';

// 挂载前注入原生文件桥：App.vue 的打开/保存即走 Tauri 对话框。
setFileHost(tauriFileHost);

createApp(App).use(Button).use(Spin).use(Tabs).use(Tooltip).mount('#app');
