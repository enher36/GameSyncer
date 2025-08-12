# GameSyncer 🎮

**Steam游戏存档云同步工具** - 一个用Rust编写的跨平台游戏存档备份和同步解决方案

## ✨ 特性

- 🎮 **智能游戏发现** - 自动检测Steam游戏和存档位置
- ☁️ **多云存储支持** - 支持腾讯云COS和Amazon S3
- 🔄 **智能同步** - 自动比较本地和云端存档，智能选择上传或下载
- 📦 **压缩存储** - 自动压缩存档文件，节省存储空间
- 🛡️ **数据安全** - SHA256校验和验证，确保数据完整性
- 📊 **详细统计** - 存储使用量、操作历史、同步状态
- 🌐 **双语支持** - 内置中文和英文界面
- 💾 **本地数据** - SQLite数据库存储配置和历史记录
- 🎨 **现代界面** - 基于egui的即时模式GUI

## 🏗️ 项目架构

GameSyncer采用Rust workspace架构，包含四个主要crate：

```
GameSyncer/
├── crates/
│   ├── core/           # 游戏发现和存档检测逻辑
│   ├── cloud/          # 云存储后端（腾讯云COS、S3）
│   ├── persistence/    # SQLite数据持久化
│   └── ui/             # egui GUI应用程序
├── docs/               # 项目文档和开发记录
└── src/bin/            # 工具程序
```

### 核心模块说明

- **`steam-cloud-sync-core`** - Steam游戏扫描、存档位置检测、手动映射管理
- **`steam-cloud-sync-cloud`** - 云存储抽象层、文件上传下载、进度跟踪
- **`steam-cloud-sync-persistence`** - 数据库管理、操作历史、配置存储
- **`steam-cloud-sync-ui`** - 用户界面、页面管理、异步任务处理

## 🚀 快速开始

### 前置要求

- Rust 1.70+ (MSVC toolchain on Windows)
- Windows SDK (Windows)
- CMake tools
- Git

### 构建项目

```bash
# 克隆仓库
git clone <repository-url>
cd GameSyncer

# 构建整个工作空间
cargo build --workspace

# 运行主应用程序
cargo run --bin steam-cloud-sync
```

### 开发环境配置

Windows环境下需要正确配置MSVC编译环境：

```bash
# 确保使用MSVC工具链
rustup default stable-x86_64-pc-windows-msvc

# 清理重建（如遇编译问题）
cargo clean
cargo build --workspace
```

详细的编译问题解决方案请参考 [COMPILE_FIX_GUIDE.md](COMPILE_FIX_GUIDE.md)

## 📖 使用说明

### 基本流程

1. **配置云存储** - 在设置页面配置腾讯云COS或AWS S3凭据
2. **扫描游戏** - 自动扫描Steam游戏库，检测存档位置
3. **同步存档** - 一键同步所有游戏存档到云端
4. **恢复存档** - 从云端下载并恢复游戏存档

### 存档检测逻辑

系统采用多层检测策略：

1. **手动映射** - 用户自定义的存档路径（最高优先级）
2. **Steam Cloud** - Steam远程存档目录
3. **已知位置** - 常见存档文件夹（Documents、AppData等）
4. **智能搜索** - 游戏安装目录递归搜索

### 云存储配置

#### 腾讯云COS
- Secret ID、Secret Key
- Bucket名称、区域

#### Amazon S3
- Access Key、Secret Key
- Bucket名称、区域

## 🛠️ 开发指南

### 代码风格

- 使用`cargo fmt`格式化代码
- 使用`cargo clippy`进行代码检查
- 遵循Rust官方命名约定

### 测试

```bash
# 运行所有测试
cargo test --workspace

# 运行特定crate测试
cargo test --package steam-cloud-sync-core

# 显示测试输出
cargo test --workspace -- --nocapture
```

### 调试

项目包含多个调试工具：

```bash
# 检查云存储内容
cargo run --bin check_cos_contents

# 调试存储统计
cargo run --bin debug_cos_storage

# 修复数据库问题
cargo run --bin fix_database
```

## 📋 待办事项

- [ ] 支持更多云存储服务（Google Drive、OneDrive）
- [ ] 增加存档版本管理
- [ ] 实现增量同步
- [ ] 添加存档自动备份计划
- [ ] 支持存档加密
- [ ] 开发Web管理界面

## 🤝 贡献指南

1. Fork本仓库
2. 创建特性分支 (`git checkout -b feature/AmazingFeature`)
3. 提交更改 (`git commit -m 'Add some AmazingFeature'`)
4. 推送到分支 (`git push origin feature/AmazingFeature`)
5. 创建Pull Request

## 📝 更新日志

### v0.1.0 (当前)
- ✅ 基础架构和核心功能
- ✅ 腾讯云COS和S3支持
- ✅ Steam游戏自动发现
- ✅ 基础GUI界面
- ✅ 中英文双语支持
- ✅ 存档压缩和校验

## 📄 许可证

[MIT License](LICENSE)

## 🔗 相关链接

- [Claude Code](https://claude.ai/code) - AI辅助开发工具
- [egui](https://github.com/emilk/egui) - Rust即时模式GUI框架
- [Steam WebAPI](https://steamcommunity.com/dev) - Steam开发者API

---

**用❤️和🦀 Rust开发** | **Built with ❤️ and 🦀 Rust**