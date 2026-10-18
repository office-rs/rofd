# 字体资产 npm 发布与 git 历史清理——rword 实施纪要（rofd 同样改造时的操作手册）

**日期：** 2026-10-17
**来源：** rword / office-rs/fonts 两仓库的发布与历史治理实操（2026-10-16~17）
**用途：** rofd 做字体切片改造会经历同样的两件事：① 把字体资产包发上 npm org；② 把商用字体从 git 历史剔除（rofd `public/fonts/` 同样躺着 6 个商业字体 ~70 MB，历史里也有）。本文记录全部流程、认证坑、CN 网络对策与强推手册，照抄即可。
**安全注意：** 全文不含任何真实 token/密码；npm token 只存在 `~/.npmrc`，操作一律用"结构检查"（计数、掩码），不在终端回显值。

---

## 1. npm org 发布（@office-rs/*）

### 1.1 认证坑（按踩坑顺序）

| 现象 | 根因 | 对策 |
|---|---|---|
| `EOTP`（publish 要求一次性验证码） | **granular token 不绕过已开 2FA 账号的 publish 校验**（官方文档暗示可以，实测不行；已用 `--userconfig` 隔离排除全局 token 串扰） | 换 **Classic Automation token**（npmjs → Access Tokens → Generate New Token → Classic → Automation）——CI/发布场景的可靠选择 |
| `E404 no permission`（PUT 阶段） | granular token 没勾目标 **org 的包读写权限**（`npm org ls <org>` 能确认 org 存在与 owner 身份，但 token scope 是另一回事） | 同上，Classic Automation 或 granular 勾全 org packages 读写 |

发布命令（Windows，node 自带 npm 被 PATH 上 npm 6 遮蔽，必须用全路径）：

```powershell
cd D:\code\fonts\packages\<pkg>
node "C:\Program Files\nodejs\node_modules\npm\bin\npm-cli.js" publish
```

### 1.2 包清单与状态（2026-10-17）

| 包 | 体量 | 状态 |
|---|---|---|
| `@office-rs/font-runtime` | 10.9 kB | ✅ 1.0.0 已上线 |
| `@office-rs/fonts-notosans` | 小 | ✅ 1.0.0 已上线 |
| `@office-rs/fonts-notosanscjksc` | **24.5 MB / 166 files** | ⏳ 网络阻断（见 §2），待发 |
| `@office-rs/fonts-notoserifcjksc` | 同级 | ⏳ 同上 |

发布前置（fonts 仓库已做完）：`woff2-encoder` 替换 wawoff2、OFL.txt 随资产包进 dist、vite peerDependency、LICENSE、`files` 白名单只含 dist。项目 `.gitignore` 加 `.npmrc`。

### 1.3 消费方收尾链（包全部上线后）

1. rword/rofd 两个 `package.json` 的 `@office-rs/font-runtime` 从 `file:../../../fonts/packages/font-runtime` 切 `^1.0.0`；
2. **用 bundled npm** 重生成 lockfile 并 install（PATH 上 npm 6 生成的 lockfile CI 会炸）；
3. 跑门：sdk vitest + web-app `vue-tsc` + e2e；
4. 提交推送；jsDelivr v1 tag 冷索引验证（大仓库摄取慢，503/502 属自愈型，用 webReader 类工具查，WebFetch 会失败）。

## 2. CN 网络现实与对策（实测结论）

- **持续大上传一律中途 RST**：npm 24.5 MB tarball、git 16 MB pack，直连与本地代理（127.0.0.1:7897）都一样——代理救小请求，救不了大上传。小增量请求（<500 KB）基本能过。
- **git 分块推送有效但会衰减**：单次 push 的 pack 控制在 ~400 KB 内成功率高；连续推送后质量下降，需要节流 + 退避。
- 大 npm 包的可靠路径：换网络（手机热点）手动发，或等网络窗口。

### 2.1 自适应分块强推手册（实测 328 提交 / ~35 分钟全量落地）

原理：把历史按"距远端 tip 的实际 pack 字节"切批，小块走 SSH 通道逐批推到一条**持久临时分支**（远端已收对象会被后续 push 复用，每次只传增量），全量落地后再原子切 main + tag。

脚本要点（完整版在 rword 会话 / 可随时重写，~60 行 bash）：

```bash
tip=$(git ls-remote origin refs/heads/tmp-rebuild | awk '{print $1}')  # 每次运行自远端取，天然可续传
for c in $(git rev-list --reverse "${tip}..main"); do
  sz=$(pack_size "$c" "$tip")          # printf '%s\n^%s\n' $c $tip | git pack-objects --revs
  if [ $sz -ge 400000 ]; then          # 大块：单独推，12 次重试 × 15s 退避
    retry_push $c 12 15 || exit 1; sleep 25   # 成功后强制冷却
  elif 批内满 15 个提交; then
    retry_push $c 6 8 || exit 1; sleep 8
  fi
  tip 更新
done
```

**三条铁律**（都踩过）：
1. **临时分支在 main + tag 落地前绝不删**——GitHub 不广播不可达对象，删了等于让服务器忘掉已收的增量；
2. 尺寸阈值按"距远端 tip 的 pack 实际字节数"算，不按提交数（10 个提交可能是 3 KB 也可能是 2.9 MB）；
3. 大小块分开协议：大块（≥400 KB）长退避 + 成功后长冷却，小块快速连发。

### 2.2 SSH 443 逃生通道（HTTPS 被掐时的 git 通路）

1. **GitHub 拒收 2048 位 RSA 老密钥**（"Key is too short"）——直接生成 ed25519：

```bash
ssh-keygen -t ed25519 -C "<email>" -f ~/.ssh/id_ed25519 -N ""
cat ~/.ssh/id_ed25519.pub   # 贴到 github.com/settings/ssh/new（Authentication Key）
```

2. `~/.ssh/config` 追加（github.com 与 ssh.github.com 共用新钥，不动可能服务其他主机的 id_rsa）：

```
Host github.com ssh.github.com
  IdentityFile ~/.ssh/id_ed25519
  IdentitiesOnly yes
```

3. 仓库只换推送地址（fetch 仍走 HTTPS，小请求不受影响）：

```bash
git remote set-url --push origin ssh://git@ssh.github.com:443/<org>/<repo>.git
ssh -T -p 443 git@ssh.github.com   # 出现 "Hi <user>!" 即通
```

注意：ssh.github.com:443 能握手不代表能传大包——§2.1 的分块协议在 SSH 上同样必需。

## 3. git 历史剔除商用字体（filter-repo 全流程）

rword 实测：`.git` 76 MB → 16 MB，18 个字体二进制（ttf/ttc/otf）跨 **3 个历史路径**（crates/web-app/public/fonts/、examples/web-app/public/fonts、examples/web-app/fonts——仓库结构调整留下的）全部剔除，只留 3 个 OFL 测试夹具。

```bash
# ① 备份（必做，重写不可逆）
git bundle create ../<repo>-pre-filter.bundle --all

# ② 每个历史路径一遍（filter-repo 一次只吃 --path 列表，路径变更史需要多轮）
git filter-repo --force --invert-paths --path crates/web-app/public/fonts
# 然后逐轮：git rev-list --all --objects | grep -iE '\.(ttf|ttc|otf)$' 查残余路径，再跑

# ③ filter-repo 会剥掉 remotes + 重写全部 hash + 重写 tag —— 重挂 origin
git remote add origin <url>

# ④ 分块强推（§2.1）：tmp-rebuild 落地 → main → tag → 删 tmp-rebuild → ls-remote 核对
```

要点：
- **所有提交 hash 全变**——本地记忆/文档里引用的旧 hash 全部作废，重要里程碑要记录新 hash 映射；
- 团队协作者必须重新 clone（或 `git fetch && git reset --hard origin/main`），旧引用推上去会带回字体对象；
- 剔除后仓库仍含 OFL 字体的地方（测试夹具）按需保留；商用字体从磁盘 `git rm --cached` 后**磁盘文件保留**做 layout fixture，但新 clone 没有（见迁移纪要 §4.6）；
- filter-repo 工具：Windows 下 `pip install git-filter-repo` 后用 `C:/Users/<user>/AppData/Local/Python/.../Scripts/git-filter-repo.exe`。

## 4. rofd 落地时的对照清单

- rofd `crates/web-app/public/fonts/`：9 只整字体（3 Noto OFL + 6 商业）→ 商业字体同 rword 姿势本地切片化 + `git rm --cached`；Noto 三只由切片通道替代后删除；
- rofd git 历史：同样含商业字体 → §3 流程走一遍（先 bundle 备份；多路径多轮；分块强推走 §2.1/§2.2）；
- npm：fonts 资产包与 font-runtime 是**共享包**，rofd 直接消费，不需要再发（除非 rofd 要发自有字体切片包，那也是 fonts 仓库加 workspace，不是 rofd 仓库发包）；
- CI/部署：rofd 若有 Pages 类公开部署，恢复自动部署前必须先解决"公开部署 = 分发商用字体"问题（rword 的 deploy-pages.yml 至今手动触发，就是这原因）。
