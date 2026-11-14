# Strinova 公开构建基线

本仓库是 Strinova 的公开构建与分发基线，保留上游 Stravia 源码、公开的
platform-plugin SDK，以及在编译前应用私有 Strinova patch 的 CI 工作流。

框架 overlay 和供应商实现保存在 private 的 MelodyAstro/Strinova patch 仓库。
公开 release 只提供 Linux x86-64 Server 与带签名摘要的 inject/plugin 安装成品，
供应商源码不属于公开开发面。

上游参考：https://github.com/Stravia-AI/StraviaPlatform

运行 native build workflow 前，请在 Actions 中配置只读的
STRINOVA_PRIVATE_TOKEN secret。
