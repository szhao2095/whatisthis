find /var/log -name '*.log' -mtime +7 -delete
rsync -avz /backup/ user@28.101.115.131:/remote-backup/
docker system prune -f
systemctl restart nginx
