# 个人版 NAS 同步

本分支基于 [quiet-md](https://github.com/s1dashu/quiet-md) 的上游提交 `57884b9c8e49d22effe18885c6d717d8acb67278`（0.1.9 源码快照）。保留 Rust + GPUI 桌面端，新增可选 NAS 同步。个人改造在 `mynote` 分支持续维护，通过 `upstream/main` 合入原项目更新；`origin` 用于推送自己的 GitHub Fork。

## 第一版能力

- Windows/macOS 共用同一套 Rust 同步引擎，离线编辑，应用运行时每 30 秒重试；设置页支持手动同步。
- 同步库内 XML、Markdown 笔记、文件夹、顺序、置顶、收藏、回收站和附件。不同笔记独立合并。
- 同一篇的并发修改保留两份，新增副本标题带 `(sync conflict)`。不做正文自动合并或实时协同。
- 清空回收站产生持久删除标记；旧设备的离线修改保留为新副本，不覆盖删除标记。陈旧的永久删除请求遇到新修改时保留新修改。
- NAS 用 SQLite 事务保存当前数据、删除标记和修订历史。重试相同冲突内容不会不断产生副本。
- 网络请求不持有文档库写锁。收到响应后先保存所有窗口，再检查磁盘是否在请求期间被修改，保留后写入的本地内容。
- 上传期间继续输入时，若 NAS 已接受本次上传的快照，客户端会推进对应修订号，并让后续输入留待下一轮上传，避免同一设备与自己产生冲突；请求期间收到另一设备的不同修改时仍保留冲突处理。
- 窗口、页宽、阅读模式、最近打开时间等设置留在当前设备。“打开本机文件”关联的外部文件不上传，界面显示跳过数量。

这是个人使用的原型：每次提交只上传变更记录，但响应仍下载整份库；单文件上限 **32 MiB**、本地附件总量上限 **64 MiB**、单次 JSON 请求/响应上限 **128 MiB**。适合先验证小型文档库，尚未实现大附件分块、增量下载、带宽优化或历史清理。两个设备同时改同一篇的元数据也可能产生冲突副本。

## 在 NAS 启动

NAS 已安装 Docker 和 Tailscale。以下命令在 NAS 上的 `services/sync` 目录运行，不是在 Windows 客户端上运行。

1. 将 `services/sync` 目录复制到 NAS。
2. 复制 `.env.example` 为 `.env`，将 `SYNC_TOKEN` 填为随机令牌。可用 `python3 -c "import secrets; print(secrets.token_hex(32))"` 生成。所有设备使用同一个令牌。
3. 启动容器：

   ```sh
   docker compose up -d --build
   docker compose logs --tail=30
   ```

4. 在 NAS 宿主机上运行 Tailscale Serve，将本机端口分享给 tailnet：

   ```sh
   tailscale serve --bg http://127.0.0.1:8787
   tailscale serve status
   ```

   默认 Compose 仅发布 NAS 的 `127.0.0.1:8787`。Serve 提供的 HTTPS 地址通常形如 `https://nas.your-tailnet.ts.net`。Serve 需要 tailnet 开启 HTTPS，访问仍受 Tailscale 访问规则控制；参见 [官方说明](https://tailscale.com/docs/features/tailscale-serve)。如果 NAS 上的 Tailscale 运行在隔离容器中，需要按该 NAS 的网络方式连接服务端口。

## 桌面连接

### 无 SSH：NAS Docker 管理界面

NAS Docker 项目管理器可以使用 `services/sync/compose.gui.yaml`。这个版本直接运行官方 `python:3.13-slim`，挂载同目录的 `server.py`，不需要在 NAS 构建镜像或运行终端命令。

将项目目录上传到 NAS，在 Docker 管理器中选择该目录，使用 GUI 版 Compose 文件创建项目，在项目环境中填写 `SYNC_TOKEN`，然后启动。容器端口 8787 映射到 NAS 的 8787；客户端地址填写 `http://<NAS的TailscaleIP>:8787`，实际地址以你自己的 NAS 为准。GUI 版也允许从 NAS 局域网地址访问该端口。它不需要更改已有 Serve/Funnel 配置。

本机 `dist/nas-docker-gui/` 提供了针对这台 NAS 的项目包：部署文件夹中的 `compose.yaml` 已写入随机生成的令牌，客户端使用的相同令牌保存在旁边的 `客户端连接信息.txt` 中。整个目录由 Git 忽略，仅供你自己的设备使用。NAS 上应保留 `compose.yaml` 与 `server.py` 的同目录关系；数据卷 `sync-data` 需随容器保留。

### 旧版 UGOS：手动创建容器

没有 Compose 项目管理的旧版 UGOS 可直接从 `python:3.13-slim` 创建 `mynote-sync` 容器。将 `存储空间1/docker/mynote-sync` 挂载到 `/app`（只读），将它的 `data` 子目录挂载到 `/data`（读写）。映射 NAS 8787 到容器 8787/TCP，环境变量添加 `SYNC_TOKEN`，值使用客户端连接信息中的令牌；`SYNC_DATA=/data`、`SYNC_HOST=0.0.0.0`、`SYNC_PORT=8787` 可保持默认。

**旧版 UGOS 的“执行命令”框按换行拆分参数，必须填写三行：**

```text
python3
-u
/app/server.py
```

不要在这个界面中写成一行，也不要加引号或空白行。界面代码 `getCmdData` 使用 `split("\n")`；单行输入会被当成完整的可执行文件名，出现 `exec: "python3 -u /app/server.py": ... no such file or directory`。这条规则只适用于旧版 UGOS 的输入框，Compose 的命令数组无需改成这种格式。

重启策略选“容器退出时重启（不考虑在进程启动时就已经停止了的容器）”，勾选“创建后启动容器”。启动后访问 `http://<NAS的TailscaleIP>:8787/health` 检查服务。

2026-11-09 已在这台 NAS 验收：Tailscale 地址的健康检查返回 HTTP 200；两份隔离临时库通过实际 Rust 引擎完成上传、下载、附件、离线冲突副本、重试、回收站、永久删除及错误令牌拒绝测试。验收使用随机 `acceptance-...` 库名，没有访问 `personal` 库。两份临时库都在当前 Windows 机器运行，macOS 编译和真机同步仍待验证。

### 连接桌面端

1. Windows 和 Mac 都连接同一个 Tailscale 网络。
2. 打开应用 → 设置 → 同步。
3. 填 NAS 的 HTTPS 地址、相同的文档库名称（如 `personal`）和令牌，点击“保存并同步”。名称只能用英文字母、数字、下划线和连字符。
4. 在另一台电脑填写相同设置。首次连接保留已有本地笔记并与远端合并；不会拿空库覆盖 NAS。

令牌与设备专属同步状态保存在本地库的 `.nas-sync/` 中。令牌文件为本机明文，Unix 上目录权限为 0700；不要将此目录复制到另一台设备或交给其他同步工具。第二台设备应通过设置页独立连接。附件暂不自动清理，避免清理仍被冲突副本或历史版本引用的文件。

## 构建与验证

Windows 需要 Visual Studio C++ Build Tools 和 Windows SDK，并从开发者终端运行；Mac 需要 Xcode Command Line Tools。工具链由 `rust-toolchain.toml` 固定。

```sh
cargo build --release --locked
cargo test --release -p fdl_core --locked
cargo build --release -p fdl_core --example nas_sync --locked
python -m unittest discover -s services/sync -v
```

`test_desktop_e2e.py` 使用两份临时库和本机 HTTP 服务，调用真正的 Rust 引擎验证附件、离线冲突、重试、回收站、永久删除和鉴权失败；没有构建示例程序时会跳过该组测试。它验证跨设备协议，两份库在当前测试机器运行，并不等于 macOS 真机验证。

开发和验收只使用临时库：

```powershell
$env:QUIET_MD_HOME = Join-Path $env:TEMP 'mynote-preview-library'
.\target\release\quiet_md.exe
```

```sh
QUIET_MD_HOME=/tmp/mynote-preview-library ./target/release/quiet_md
```

## 备份与恢复

NAS 的完整状态保存在 `/data/sync.sqlite3`。Compose 方案对应 Docker named volume `sync-data`；旧版 UGOS 手动挂载方案对应 `存储空间1/docker/mynote-sync/data`。备份时先通过 Docker 界面停止 `mynote-sync` 容器（Compose 也可用 `docker compose stop`），再备份整个数据目录或卷；恢复整个目录或卷后再启动。也可用 SQLite backup API 在线备份，不要只复制正在写入的数据库主文件而遗漏 WAL。

每次替换或移走本地文档前，旧内容还会保存在 `.nas-sync/recovery/<uuid>.json`。每条 `doc/<id>` 记录的 `body` 是原 XML 或 Markdown，`meta` 是目录元数据，`deleted` 表示回收站状态；手工恢复前先复制整个本地库。NAS `history` 表保留各修订的 JSON，暂未提供界面化历史浏览或一键恢复。

服务检测数据库标识。如果已连接设备遇到新建的空 NAS 数据库，会拒绝继续同步；应先恢复 NAS 备份。需要建立全新同步关系时，先备份本地库，再使用新的同步文档库名称。不要直接删除同步状态来处理普通连接故障。

个人更新渠道使用自己的 GitHub Releases，配置、首次安装与发布步骤见 [个人版本的自动更新](PERSONAL_UPDATES.md)。正式对外分发时仍应检查上游名称、图标与素材的使用说明，见 README 和 THIRD_PARTY.md。

## 维护个人版本

个人仓库为 [yqhuang2912/quiet-md](https://github.com/yqhuang2912/quiet-md)，默认分支为 `mynote`。`origin` 指向个人仓库；`upstream` 指向 `https://github.com/s1dashu/quiet-md.git`。个人改造提交到 `mynote`，`main` 保留原项目基线。日常修改后先检查 `git diff` 和 `git status`，确认都是本次要保存的文件，再提交并推送：

```sh
git switch mynote
git add .
git commit -m "feat: describe the change"
git push origin mynote
```

引入原项目更新前，先提交手头修改，确认 `git status` 干净。在已配置好 Rust 和 Windows SDK（Mac 为 Xcode Command Line Tools）的构建环境中运行：

```sh
git switch mynote
git fetch upstream
git merge --no-commit --no-ff upstream/main
python scripts/check.py
git commit -m "merge: sync upstream updates"
git push origin mynote
```

如果合并提示 `Already up to date`，无需创建合并提交。出现冲突时先解决冲突、`git add` 对应文件，再运行检查和提交；想放弃尚未提交的这次合并，可运行 `git merge --abort`。macOS 可将 `python` 换成 `python3`。完整检查通过后再构建和更新正在使用的客户端；源码合并不会自动更新 NAS 容器或已打开的桌面应用。

当前 Windows 仓库的 GitHub 网络访问使用本机已启用的代理 `127.0.0.1:7897`，仅写在本地 `.git/config`；其他电脑应使用各自的网络配置。NAS 令牌、`.env` 和 `dist/` 打包文件不提交到 Git。
