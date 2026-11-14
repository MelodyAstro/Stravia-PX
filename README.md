# Strinova public build base

This repository is the public build and distribution base for Strinova. It
contains the upstream Stravia source, the public platform-plugin SDK, and the
CI workflow that applies the private Strinova patch before compiling release
artifacts.

The framework overlay and vendor implementations live in the private
MelodyAstro/Strinova patch repository. Public releases contain installable
Linux x86-64 server and signed inject/plugin artifacts; vendor source is not
part of this repository's public development surface.

Reference upstream: https://github.com/Stravia-AI/StraviaPlatform

Set the STRINOVA_PRIVATE_TOKEN Actions secret to a read-only token for the
private patch repository before running the native build workflow.
