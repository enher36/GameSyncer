# CLAUDE.md

本文件为Claude Code (claude.ai/code)在此代码仓库中工作时提供指导。

**重要说明：在此项目中，Claude应默认使用中文与用户对话，除非用户明确要求使用其他语言。**

## 项目概览

GameSyncer是一个用Rust编写的Steam游戏存档同步工具，允许用户将游戏存档备份和同步到云存储（腾讯云COS或Amazon S3）。项目采用工作空间架构，包含四个主要crate：

- `steam-cloud-sync-core`: 游戏发现和存档检测逻辑
- `steam-cloud-sync-cloud`: 云存储后端（腾讯云COS、S3）
- `steam-cloud-sync-persistence`: 基于SQLite的数据持久化
- `steam-cloud-sync-ui`: 基于egui的GUI应用程序

## 构建和开发命令

### 构建项目
```bash
# 构建整个工作空间
cargo build --workspace

# 构建特定crate
cargo build --package steam-cloud-sync-core
cargo build --package steam-cloud-sync-cloud  
cargo build --package steam-cloud-sync-persistence
cargo build --package steam-cloud-sync-ui

# 构建发布版本
cargo build --workspace --release
```

### 运行应用程序
```bash
# 运行主GUI应用程序
cargo run --bin steam-cloud-sync

# 带特定功能或参数运行
cargo run --bin steam-cloud-sync -- [args]
```

### 测试
```bash
# 运行所有测试
cargo test --workspace

# 运行特定crate的测试
cargo test --package steam-cloud-sync-core
cargo test --package steam-cloud-sync-cloud
cargo test --package steam-cloud-sync-persistence
cargo test --package steam-cloud-sync-ui

# 运行特定测试
cargo test --package steam-cloud-sync-core test_sanitize_game_name
cargo test --package steam-cloud-sync-cloud test_compression

# 运行测试并显示输出
cargo test --workspace -- --nocapture
```

### 代码质量检查
```bash
# 检查代码但不构建
cargo check --workspace

# 快速检查特定包
cargo check --package steam-cloud-sync-ui --quiet

# 格式化代码
cargo fmt --all

# 代码检查（如果配置了clippy）
cargo clippy --workspace
```

## Windows编译环境配置

项目依赖需要C++编译的库（aws-lc-sys, ring, zstd-sys）。如果遇到编译错误：

### 必需工具
- Visual Studio Build Tools 2022 或 Visual Studio Community
- Windows SDK (10/11)
- CMake tools
- 确保使用MSVC工具链：`rustup default stable-x86_64-pc-windows-msvc`

### 编译故障排除
```bash
# 清理重建
cargo clean
cargo build --workspace

# 以管理员权限运行（如需要）
# 设置环境变量解决权限问题
set RUSTFLAGS="-C target-feature=+crt-static"
```

## 架构概览

### 核心模块 (`crates/core`)
- **Steam集成**: 使用`steamlocate` crate发现Steam游戏和库
- **存档检测**: 多层启发式方法：
  1. Steam Cloud远程目录 (`steamdir/userdata/*/appid/remote`)
  2. 已知系统文件夹 (文档/我的游戏、AppData等)
  3. 游戏安装目录递归搜索
- **手动映射**: 基于JSON的用户自定义存档路径存储，位于`~/.steam-cloud-sync/mappings.json`

### 云模块 (`crates/cloud`)
- **后端抽象**: `CloudBackend` trait支持多个存储提供商
- **腾讯云COS后端**: 自定义HTTP客户端，使用签名v1认证
- **S3后端**: 使用AWS SDK，支持分片上传
- **数据管理**: ZIP压缩、SHA256校验和、断点续传
- **存储分析**: 用户和存储桶级别的存储统计
- **游戏映射**: `game_mapping.rs`处理历史存档文件的游戏名称到app_id映射

### 持久化模块 (`crates/persistence`)
- **SQLite数据库**: 位于`%APPDATA%/SteamCloudSync/steam-cloud-sync.db`
- **模型**: 云操作、同步会话、游戏配置、应用设置、云统计
- **管理器**: 云历史记录和配置的独立存储
- **类型安全**: 使用sqlx的编译时检查和类型化查询

### UI模块 (`crates/ui`)
- **egui框架**: 即时模式GUI，支持自定义样式
- **本地化**: 通过`LocalizationManager`内置中英文支持
- **页面架构**: 模块化页面系统（主页、历史记录、设置、云存档）
- **异步集成**: tokio运行时用于后台操作
- **字体支持**: Windows上的中文字符系统字体加载
- **消息传递**: UI使用mpsc通道进行异步任务通信

## 关键设计模式

### 异步架构
- UI操作为I/O密集型工作生成tokio任务
- 通过UI和后台任务之间的`mpsc`通道进行进度跟踪
- `AppViewModel`集中管理状态和异步操作
- ServiceManager作为业务逻辑和云服务的中间层

### 错误处理
- 使用`anyhow::Result`进行灵活的错误传播
- 核心模块中的自定义错误类型（`ScanError`）
- UI显示用户友好的错误消息
- 云操作失败时的详细日志记录

### 配置管理
- 设置存储在SQLite中，采用类型化配置系统
- 云后端凭据与应用逻辑分离管理
- 多用户存档分离的用户ID系统
- UI消息系统用于设置更新（如默认下载位置）

### 存档检测逻辑
多层存档检测系统的优先级：
1. 手动映射（最高优先级）
2. Steam Cloud远程文件夹
3. 已知文件夹中的模糊匹配
4. 安装目录中的启发式搜索

存档验证包括修改时间检查（30天内）和最小文件数量要求。

## 重要开发注意事项

### 云存储实现细节
- **校验和处理**: 上传时使用SHA256，下载时智能处理不同格式（MD5/ETag/SHA256）
- **文件压缩**: 所有存档都压缩为ZIP格式上传
- **下载解压**: 自动解压到游戏存档目录，或保存ZIP到自定义位置
- **路径安全**: 用户ID和文件名都经过清理以确保路径安全

### 测试策略
- 每个crate中的单元测试专注于核心逻辑
- 集成测试使用临时目录和模拟数据
- 异步测试使用`tokio::test`宏
- 云后端测试需要模拟服务器（`wiremock`）
- 创建简单的Rust程序来测试特定功能

### 调试工具和故障排除
- 使用详细的调试输出（eprintln!宏）追踪云操作
- XML解析过程包含逐步调试信息
- 校验和不匹配时提供详细的比较信息
- 可以创建独立的调试程序来测试特定问题

### 平台考虑
- Windows特定的中文支持字体加载
- 使用`PathBuf`进行跨平台路径处理
- Steam检测回退的Windows注册表访问

### 安全性
- 不应提交API密钥或凭据
- 用户凭据在SQLite中加密/安全存储
- 云操作使用适当的认证头

### 性能优化
- UI中大型游戏列表的虚拟滚动
- 每个游戏云存档的延迟加载
- 带进度反馈的后台扫描
- 带宽优化的ZIP压缩

### 常见问题解决
- **编译错误**: 参考COMPILE_FIX_GUIDE.md，通常需要正确配置MSVC工具链
- **云存档不显示**: 检查腾讯云COS的XML解析逻辑和用户ID匹配
- **校验和失败**: 现代实现会继续下载但记录警告，无需中断
- **UI状态问题**: 确保消息通道正确设置且ServiceManager已初始化

## 开发工作流程和调试

### 创建调试工具
当遇到特定问题时，可以创建简单的独立Rust程序来测试和验证：
```rust
// 示例：创建test_xxx.rs文件来调试特定功能
fn main() {
    println!("调试特定功能...");
    // 测试逻辑
}
```

### 常用调试命令
```bash
# 编译并运行调试工具
rustc test_debug.rs && ./test_debug.exe

# 清理临时文件
rm test_debug.rs test_debug.exe
```

### 任务管理
使用TodoWrite工具跟踪开发进度，特别是复杂的多步骤任务。对于简单的单步任务可以直接完成。