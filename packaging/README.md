# Native desktop packaging

`scripts/build-app-release.sh` owns the first-party desktop artifacts:

- macOS: `Zode.app` plus a `.tar.gz`, dependency-audited and ad-hoc signed.
- Windows: a portable `.zip` and WiX v4 `.msi`; both are explicitly unsigned.
- Linux: a `linuxdeploy` AppImage plus a portable `.tar.gz`.

Release artifact names use the `zode-desktop-<version>-<target>` prefix. The
application bundle stays `Zode.app`, and the executable stays `zode-app` (or
`zode-app.exe` on Windows).

The Linux builder downloads linuxdeploy release `1-alpha-20251107-1` from the
upstream GitHub release and verifies the architecture-specific SHA-256 before
execution. The Windows installer keeps one stable UpgradeCode and maps SemVer
to the three numeric fields supported by MSI.

These are packaging contracts, not claims of notarization, Authenticode, or
distribution-channel trust. A future release workflow can add identity-backed
signing as a separate, auditable stage.
