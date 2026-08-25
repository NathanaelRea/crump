# Clean-account portability receipt — 2026-08-25

This is repository/offline portability evidence only. It is not producer-listening, PipeWire-xrun, realtime-scheduling, or named-hardware evidence.

## Bound inputs

- Script: `tests/qualification/clean-account-portability.sh`
- Script SHA-256: `1dbd7f62cb3ee073e7da1509f457eafd744096273de8f20de07188ee7d12c351`
- Executable SHA-256: `83fc728afe8e49e2a731f2fdcde511ba7cfd2a92185d928d8a1dff948ee0ce9b` (release executable extracted from the Debian package candidate)
- Maximum fixture SHA-256: `5e3bc0506cf38d70b15b55a8fb589ace9d4ca23a552e38601abb8717df92ac3c`
- Base image: `archlinux:base@sha256:b860afd5823683f7ea389ba5f00d812f4fe55f6f286dea329d2abeefa535e309`
- Image ID: `sha256:5450f1f6c02a03b7675390a41f7b082f6f24c8737f1c583d0d23c49c0f791efe`
- Image OS identity: Arch Linux `20260823.0.578598`
- Container network: disabled (`--network none`)
- Repository mount: read-only (`/repo:ro`)
- Dynamic-loader prerequisite: host `libpipewire-0.3.so.0.1608.0`, SHA-256 `3165362fc7fc71ca6b2d01523038c0b3539c27bf5182fafaa91e3e9f9195cf30`, was mounted read-only at `/usr/lib/libpipewire-0.3.so.0` because the base image does not include that library. No PipeWire server or device was used.

## Invocation

```sh
package_root=/tmp/crump-deb-final-extract/root

docker run --rm --network none \
  -v "$PWD:/repo:ro" \
  -v "$package_root/usr/bin/crump:/opt/crump:ro" \
  -v /usr/lib/libpipewire-0.3.so.0:/usr/lib/libpipewire-0.3.so.0:ro \
  archlinux:base@sha256:b860afd5823683f7ea389ba5f00d812f4fe55f6f286dea329d2abeefa535e309 \
  /repo/tests/qualification/clean-account-portability.sh /opt/crump
```

The script created a distinct OS user with an empty home/XDG environment, copied the complete source/assets/Active History, ran validation/status/inspection and non-mutating playback/render plans, exported/inspected/previewed a module, rendered and byte-compared the transfer, edited and rendered it, restored History entry `000000000001`, and rendered and byte-compared the restoration.

## Result

```text
Rendered 96240 frames to …/clean-project/modified/master.wav
WAV SHA-256: baf10d8d21b6682968092c77500ed55ed738267a741e879d47b862870efe81c6
Restored 000000000001 as new history entry 000000000003
Rendered 96240 frames to …/clean-project/restored/master.wav
WAV SHA-256: 2a643eb1bf785d5a13e220817448c3097626184cc5ad28067d28e83908e79354
clean-account portability: PASS
```

The differing modified/restored hashes are expected; the script additionally performed exact equality checks between baseline/transferred and baseline/restored master WAV and manifest pairs before printing `PASS`.
