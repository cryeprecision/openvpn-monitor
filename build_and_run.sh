# this file is called from `sync.sh` and is run on the build server in the project directory!
echo '--> building project' && cargo build --release --target x86_64-unknown-freebsd && \
    echo '--> killing old processes' && ssh root@10.0.0.1 'bash -c "killall openvpn-monitor; while pgrep openvpn-monitor > /dev/null; do sleep 1; done;"' && \
    echo '--> copying executable' && scp target/x86_64-unknown-freebsd/release/openvpn-monitor root@10.0.0.1:/usr/local/bin && \
    echo '--> running executable on server' && ssh root@10.0.0.1 'bash -c "nohup openvpn-monitor >> openvpn-monitor.out 2>&1 &"' && \
    echo '--> done!'
