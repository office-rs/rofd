// rofd web app - Vue 3 + Ant Design Vue 宿主，界面复刻参考 OFD 阅读器。
//
// SDK（Editor.init）持有 canvas、WebGPU 初始化、字体加载、DOM 事件绑定与
// 渲染循环；本应用提供功能区页签（阅读/注释/编辑/签章/票据）、
// 状态栏（页码/缩放比）与右键批注删除菜单。
//
// antd 按需注册（只 use 用到的组件），避免全量引入把包体撑到 1.4MB。

import { createApp } from "vue";
import { Button, Spin, Tabs, Tooltip } from "ant-design-vue";
import App from "./App.vue";
import "ant-design-vue/dist/reset.css";

createApp(App).use(Button).use(Spin).use(Tabs).use(Tooltip).mount("#app");
