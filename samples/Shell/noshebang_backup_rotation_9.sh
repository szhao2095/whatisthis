DATE=$(date +%Y%m%d)
SRC=/var/www
DST=/backup/snapshot-$DATE
mkdir -p $DST
rsync -a --delete $SRC/ $DST/
find /backup -maxdepth 1 -name 'snapshot-*' -mtime +56 -exec rm -rf {} +
echo "backup done at $(date)" >> /var/log/backup.log
