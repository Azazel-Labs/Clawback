# Running Clawback in Docker

The Docker image runs the Linux desktop app on a server and streams it to your browser with [Selkies](https://docs.linuxserver.io/images/docker-baseimage-selkies/). It is for NAS boxes and home servers: map the folders you want to look at into the container, open a browser tab, and see where their space went. Anything mapped read/write can be deleted from the map.

## Start it

Save this as `compose.yaml` and run `docker compose up -d`:

```yaml
services:
  clawback:
    image: ghcr.io/azazel-labs/clawback:latest
    container_name: clawback
    shm_size: 1gb
    ports:
      - 3001:3001
    environment:
      - PUID=1000
      - PGID=1000
      - TZ=Etc/UTC
      - CUSTOM_USER=clawback
      - PASSWORD=change-me
    volumes:
      - ./config:/config
      - /srv/media:/data/media
      - /srv/downloads:/data/downloads
      - /mnt/backups:/data/backups:ro
    restart: unless-stopped
```

Then open `https://<server>:3001` and accept the self-signed certificate. The same file, with comments, is in [packaging/docker/compose.yaml](../packaging/docker/compose.yaml).

To update, pull the new image and recreate the container. Settings in `/config` are kept.

```sh
docker compose pull && docker compose up -d
```

Images are published for x86-64 and ARM64:

| Tag | Contents |
| --- | --- |
| `latest` | The current stable release. |
| `0.3`, `0.3.0` | A specific release line or version. |
| `nightly` | The latest commit on `main`, rebuilt nightly when it changes. |

To build the image yourself, run `docker build -f packaging/docker/Dockerfile -t clawback-web .` from the repository root.

## With TSDProxy

[TSDProxy](https://almeidapaulopt.github.io/tsdproxy/) puts the container on your tailnet with its own name and a real HTTPS certificate. Point it at port 3000, where the image serves plain HTTP for a proxy, and drop the published port:

```yaml
services:
  clawback:
    image: ghcr.io/azazel-labs/clawback:latest
    container_name: clawback
    shm_size: 1gb
    environment:
      - PUID=1000
      - PGID=1000
      - TZ=Etc/UTC
    volumes:
      - ./config:/config
      - /srv/media:/data/media
      - /mnt/backups:/data/backups:ro
    labels:
      tsdproxy.enable: "true"
      tsdproxy.name: clawback
      tsdproxy.port.1: "443/https:3000/http"
    restart: unless-stopped
```

Then open `https://clawback.<your-tailnet>.ts.net`. Tailscale handles who can reach it, so the `PASSWORD` login is optional here. Keep it if others share your tailnet. Don't add `tailscale_funnel` to the port: that would put the page, and its Delete command, on the public internet.

## GPU acceleration

A GPU is optional. Without one, the map is drawn in software and the stream is encoded on the CPU, which works fine for a disk map. If you pass a GPU through, Selkies detects it at startup and uses it for both drawing and video encoding, and Clawback renders on it too. Nothing else needs setting.

**Intel or AMD:** add the render devices to the service.

```yaml
    devices:
      - /dev/dri:/dev/dri
```

On a host with several GPUs, run `ls /dev/dri` and set `DRINODE` (drawing) and `DRI_NODE` (encoding) to the render node to use, such as `/dev/dri/renderD128`.

**NVIDIA:** install the [NVIDIA Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html) on the host and run `sudo nvidia-ctk runtime configure --runtime=docker` once. Then reserve the GPU in the service; `graphics` is the capability that matters for drawing:

```yaml
    deploy:
      resources:
        reservations:
          devices:
            - driver: nvidia
              count: 1
              capabilities: [compute, video, graphics, utility]
```

In Portainer, paste either block into the stack's Compose file. The GPU toggle on the container editor's **Runtime & Resources** tab does the same for NVIDIA. NVIDIA needs host driver 595.80 or newer, or extra kernel settings for older drivers; see the [Selkies GPU guide](https://docs.linuxserver.io/selkies/user-guide/gpu/). To go back to software drawing, remove the block.

## Choosing folders

Map each folder under `/data`. Clawback opens `/data` when it starts and shows every mapped folder on one map. **File → Open folder** lists each mapped folder separately, with its size and free space.

- **Read/write** (`/srv/media:/data/media`): you can browse the folder and delete from it.
- **Read-only** (`/mnt/backups:/data/backups:ro`): you can browse the folder, but Clawback never offers to delete from it. The folder picker marks it **Read-Only**.

Network shares mounted on the host, or mapped as Docker NFS or CIFS volumes, work the same way.

## Deleting

The container has no Recycle Bin or Trash, so every delete is permanent. Clawback asks for confirmation each time and shows how much will be removed.

Clawback runs as the user and group set by `PUID` and `PGID`. It can only delete files that user can delete on the host. Set both to the owner of the mapped folders (run `id <user>` on the host to find the numbers).

To turn deletion off for the whole container, set `CLAWBACK_ALLOW_DELETE=false`. Clawback then works as a viewer only, and the Settings option that re-enables it is locked. That option is saved in `/config`, so if you later allow deletion again, also turn off **Settings → Disable "Delete" Command**.

## Security

Anyone who can open the page can delete everything mapped read/write.

- Always set `PASSWORD` (and optionally `CUSTOM_USER`, which defaults to `abc`). Without a password there is no login at all.
- That login is basic HTTP authentication, adequate for a trusted home network only. Don't expose the port to the internet. Use [TSDProxy](#with-tsdproxy) or another VPN, or a reverse proxy with proper authentication; port 3000 serves plain HTTP for a proxy that terminates TLS.
- The image is locked down as a single-app kiosk by default (`HARDEN_DESKTOP=true`, `HARDEN_OPENBOX=true`): no terminal, no file transfers, no `sudo`, and Clawback restarts if it is closed. See the [Selkies security guide](https://docs.linuxserver.io/selkies/user-guide/security/) before relaxing this.

## Options

| Variable | Default | Meaning |
| --- | --- | --- |
| `PUID`, `PGID` | `1000` | User and group Clawback runs as. |
| `CUSTOM_USER`, `PASSWORD` | unset | Browser login. Set a password. |
| `CLAWBACK_ALLOW_DELETE` | `true` | `false` hides deletion entirely. |
| `CLAWBACK_DATA` | `/data` | Folder holding the mapped volumes, opened at startup. |
| `TZ` | `Etc/UTC` | Time zone for file dates. |

Settings, recent folders, and snapshots are saved in `/config`. Map it to keep them when the container is updated. The remaining Selkies options are in its [documentation](https://docs.linuxserver.io/images/docker-baseimage-selkies/).

## Updates

Release images check GitHub once at startup for a newer version, sending nothing about you or your files. When one is out, a small notice appears at the right of the menu bar; click it to copy `docker compose pull && docker compose up -d`. Turn the check off in **Settings → Updates**. Images you build yourself never check.

## Limitations

- **Without a GPU**, the map is drawn in software, so very large maps redraw more slowly than on a desktop. See [GPU acceleration](#gpu-acceleration).
- **Live updates** come from inotify. They see changes made on a Linux host, but not changes made outside the container on Docker Desktop (Windows and macOS) or on network shares. Use **File → Rescan** there.
- **Open** and **Show in file manager** are hidden; there is no desktop to open files on.
- Hard links that span two mapped folders are counted twice, because the container sees them as separate filesystems.
