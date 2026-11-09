# 个人版本的自动更新

安装包来自 [yqhuang2912/quiet-md 的 GitHub Releases](https://github.com/yqhuang2912/quiet-md/releases)，更新清单保存在同一仓库独立的 `updates` 分支，通过 raw.githubusercontent.com 读取。客户端只接受该仓库对应版本和平台的安装包。原作者的 CDN 和更新公钥不参与个人版本的更新。NAS 同步服务无需修改，也不保存安装包。

客户端启动 10 秒后检查一次，之后每 12 小时检查；也可在「设置 → 关于」手动检查。自动检查可以在「设置 → 通用」关闭。发现新版本后，由用户选择下载和重启安装，退出前仍检查笔记是否保存。没有公开 Release 或对应平台的清单时，显示「暂无已发布更新」。

## 第一次安装

0.2.0 的清单地址固定在 Release 附件内，迁移后需要手动安装一次 0.2.1 或更高版本。新版会读取独立更新分支，之后恢复应用内更新。笔记和 NAS 配置不受更新清单迁移影响。自定义笔记库位置见[文档库位置](LIBRARY_LOCATION.md)。

Windows 从个人 Release 下载 `quiet-md-<版本>-windows-x64-setup.exe` 并安装一次；以后即可在应用中更新。旧的开发用单文件 EXE 不具备安装信息，需要手动完成这次安装。个人安装包使用独立 AppId、安装目录 `Programs/MyNote` 和快捷方式「Quiet.md MyNote」。笔记仍使用现有 Quiet.md 文档库，已有 NAS 同步配置不变。

macOS 只发布 Apple Silicon（M 系列）ZIP，将 `quiet.md.app` 安装到可写的应用目录。个人包默认使用 ad-hoc 签名，并由个人 Minisign 密钥验证更新；没有 Apple Developer ID 公证，首次运行仍可能出现系统确认。具备自己的 Developer ID 和公证配置后，可将 `scripts/release.sh` 的正式签名流程用于发布。已有 Developer ID 应用的更新仍要求相同 Apple 团队。

## 发布流程

工作流为 `.github/workflows/personal-release.yml`，只构建 Windows x64 和 macOS Apple Silicon，不构建 Intel Mac。签名和校验和仍在构建中生成并验证；Release 只上传两个安装包。两个包上传完成并公开后，`scripts/publish-update-manifests.py` 验证对应的公开安装包及大小，再以一次 Git 提交原子更新 `updates` 分支的两份清单。草稿不会更新公开渠道，旧工作流也不能将渠道回退到更低版本。分支初次创建时只有两份清单，不含源码或密钥；更新采用快进，不强制覆盖并发发布。

GitHub 自动附带的 Source code (zip) 和 Source code (tar.gz) 不属于手动上传的附件，不能通过删除 Release assets 移除；页面会显示两个安装包和两个源码下载链接。

正常发布新版本：

1. 在个人分支更新根目录和 `crates/core/Cargo.toml` 的版本，更新 `Cargo.lock` 与更新日志，运行 `python scripts/check.py`。
2. 检查改动，提交并推送到个人分支。
3. 给同一个提交推送与版本完全一致的标签，例如 `mynote-v0.2.1`。不要使用原项目的 `v*` 标签发布个人安装包。
4. 工作流通过两平台检查、打包和签名后，自动公开这个版本。已发布的版本不能覆盖，应增加版本号重新发布。

合并原项目更新时，保留个人版本号、更新公钥、仓库地址和应用标识；上游版本号只用于记录源码来源，不应覆盖个人发布序列。

也可以在 GitHub Actions 手动运行「Personal release」：默认只准备草稿，勾选 `publish` 才公开。第一次推送发布标签或从 Actions 发布之前，应确认该提交包含所有需要交付的个人改动。

若构建成功、仅发布阶段失败，可在手动运行时填写 `packages_run_id`，复用该次构建的安装包，不必重新编译。`scripts/release-source.py` 会检查两个构建任务成功、构建提交与发布标签一致，并确认更新公钥一致。签名工具从 Minisign 官方的固定版本下载并校验 SHA-256，不依赖 Ubuntu 软件源是否提供此包。

清单的固定检查地址为：

```text
https://raw.githubusercontent.com/yqhuang2912/quiet-md/updates/update-windows-x64.json
https://raw.githubusercontent.com/yqhuang2912/quiet-md/updates/update-macos-arm64.json
```

每份清单中的安装包 URL 固定到 `mynote-v<版本>`，包含版本、大小和完整 Minisign 签名。客户端下载后校验大小、签名、公钥和签名中的文件名，拒绝损坏、冒充版本或来自其他仓库的更新包。

## 签名密钥

GitHub 仓库 Secret `MYNOTE_UPDATE_SECRET_KEY` 用于发布签名，公钥在 `packaging/mynote-update-key.pub`，编译进客户端。私钥的本机备份在 `%USERPROFILE%\.mynote-signing\updates.key`，位于源码仓库之外；不要提交或公开私钥，并另作安全备份。后续发布继续使用同一密钥，随意换公钥会使已有客户端拒绝更新。

`scripts/prepare-updates.py` 会先校验所有平台安装包及其打包校验和，再签名、验证签名并生成清单。签名错误或文件缺失会让发布失败。`scripts/test_updates.py` 使用临时测试密钥覆盖文件篡改、错误公钥和清单 URL，不使用发布私钥。

## 本地验证

```powershell
python scripts/check-release-tag.py
python scripts/check.py
```

已安装 Minisign 时，发布签名测试会实际生成临时密钥并验证；否则这组测试跳过，GitHub 发布工作流会安装 Minisign 后再执行。可设置 `MYNOTE_MINISIGN` 为本机工具路径。

`QUIET_MD_UPDATE_URL` 只用于隔离测试：指定清单目录会允许开发版检查、支持本机 HTTP 测试源。正式发布不要设置此环境变量。运行预览或测试一律使用 `QUIET_MD_HOME` 指向临时文档库，不能对真实笔记库做更新安装测试。
