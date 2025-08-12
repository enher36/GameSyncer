# Windows编译环境配置指南

## 问题说明
GameSyncer项目依赖一些需要C++编译的库（aws-lc-sys, ring, zstd-sys），在Windows上需要正确配置构建环境。

## 解决方案

### 选项1: 安装Visual Studio Build Tools
1. 下载并安装 "Visual Studio Installer"
2. 选择 "Visual Studio Build Tools 2022"
3. 在工作负载中选择：
   - "C++ build tools" 
   - "Windows 10/11 SDK"
   - "CMake tools for C++"
   - "NASM" (可选，用于优化)

### 选项2: 使用rustup设置工具链
```bash
# 确保使用MSVC工具链
rustup default stable-x86_64-pc-windows-msvc

# 安装必要组件
rustup component add rust-src
```

### 选项3: 设置环境变量
```bash
# 设置构建相关环境变量
set RUSTFLAGS="-C target-feature=+crt-static"
set CARGO_TARGET_DIR=D:\tmp\cargo-target
```

### 选项4: 清理并重建
```bash
# 清理所有编译缓存
cargo clean
rd /s /q target
rd /s /q %USERPROFILE%\.cargo\registry\cache

# 重新编译
cargo build --release
```

### 选项5: 运行管理员权限
- 以管理员身份运行命令提示符
- 在管理员命令提示符中运行cargo命令

## 测试编译
```bash
# 测试编译各个包
cargo check --package steam-cloud-sync-core
cargo check --package steam-cloud-sync-persistence  
cargo check --package steam-cloud-sync-cloud
cargo check --package steam-cloud-sync-ui

# 如果单个包成功，尝试整个工作空间
cargo build --workspace
```

## 快速验证修复
如果编译成功，运行以下命令测试我们的修复：

```bash
# 测试数据库修复工具
cargo run --bin fix_database

# 运行主程序
cargo run --bin steam-cloud-sync
```

应该看到：
- History页面显示游戏名称而不是ID
- 文件大小正确显示
- Cloud saves页面显示存档记录
- 详细的调试日志输出

## 如果仍有问题
如果编译问题持续，可以考虑：
1. 使用WSL2 + Linux环境开发
2. 使用Docker容器编译
3. 暂时禁用某些功能特性