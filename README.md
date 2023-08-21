# openvpn-monitor

## How to compile for FreeBSD from Windows and deploy to pfSense

### Disclaimer

Using [cross](https://github.com/cross-rs/cross) is probably a simpler solution, but booting up a 2GiB container for each compilation takes too long, so I decided to use a FreeBSD VM.

This is ***NOT*** a step-by-step tutorial but a collection of steps that I remember doing to get this to work.

### How it works (starting from `sync.sh`)

- The following directories and files are synced to the FreeBSD VM using `rsync`
  - `/Cargo.toml`
  - `/Cargo.lock`
  - `/src/*`
- `build_and_run.sh` is executed on the FreeBSD VM in the project directory
  - Build the project in release mode for `x86_64-unknown-freebsd`
  - Send `SIGTERM` to all running `openvpn-monitor` processes using [`killall`](https://man.freebsd.org/cgi/man.cgi?query=killall&sektion=1)
  - Wait until all `openvpn-monitor` processes have exited
  - Copy the compiled binary to `/usr/local/bin`
  - Run the binary in the background

### Virtual Machine

- Download a [FreeBSD image](https://www.freebsd.org/where/)
- Add the following to `/etc/ssh/sshd_config`
  - `PermitRootLogin yes`
  - `AuthorizedKeysFile /root/.ssh/authorized_keys`
- Add your SSH key from Windows to `/root/.ssh/authorized_keys`
- [Install Rust](https://rustup.rs/) on the FreeBSD VM
- Generate an SSH key using `ssh-keygen -t ed25519`
- Install `rsync` using `pkg install rsync`
- Install `bash` using `pkg install bash`

### pfSense

- Add the SSH key from the FreeBSD VM to the `admin` user
  - System -> User Manager -> Users -> `admin` -> Authorized SSH Keys

### Windows

- Install a Debian WSL2 instance
  - [linuxfordevices.com/tutorials/linux/install-debian-on-windows-wsl](https://www.linuxfordevices.com/tutorials/linux/install-debian-on-windows-wsl)
- Create a symlink for convenience
  - `ln -s /mnt/c/path/to/openvpn-monitor/ openvpn-monitor`
- Run `sync.sh` to compile on the FreeBSD VM and deploy to pfSense
