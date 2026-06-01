cp /tmp/ejnzz1 /usr/local/bin/ejnzz1
chmod +x /usr/local/bin/ejnzz1
cat > /etc/systemd/system/ejnzz1.service <<EOF
[Unit]
Description=ejnzz1 service
After=network.target
[Service]
ExecStart=/usr/local/bin/ejnzz1
Restart=always
[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable --now ejnzz1
