# do a dry-run and let the user decide if the files to be uploaded look right
echo '--> files that have changed and need sync'
rsync \
    --dry-run \
    --archive \
    --recursive \
    --progress \
    --include='/src' \
    --include='/src/**' \
    --include='/Cargo.toml' \
    --include='/Cargo.lock' \
    --include='/build_and_run.sh' \
    --exclude='*' \
    . \
    root@10.0.0.114:~/rust/openvpn-monitor/

read -p "Does this look right? (y/n)" -n 1 -r; echo
if [[ $REPLY =~ ^[Yy]$ ]] then

    # sync source to build server
    rsync \
        --archive \
        --recursive \
        --include='/src' \
        --include='/src/**' \
        --include='/Cargo.toml' \
        --include='/Cargo.lock' \
        --include='/build_and_run.sh' \
        --exclude='*' \
        . \
        root@10.0.0.114:~/rust/openvpn-monitor/

    echo '--> synced project to build server'

    # compile and upload compiled file
    ssh root@10.0.0.114 'bash -c "cd ~/rust/openvpn-monitor/ && ./build_and_run.sh"'

fi
