#!/bin/sh
set -eu
fixture_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
pki_dir="$fixture_dir/pki"
rm -rf "$pki_dir"
mkdir -p "$pki_dir/root/newcerts" "$pki_dir/intermediate/newcerts"
: > "$pki_dir/root/index.txt"; printf '1000\n' > "$pki_dir/root/serial"; printf '1000\n' > "$pki_dir/root/crlnumber"
: > "$pki_dir/intermediate/index.txt"; printf '2000\n' > "$pki_dir/intermediate/serial"; printf '2000\n' > "$pki_dir/intermediate/crlnumber"
cat > "$pki_dir/root/openssl.cnf" <<CONF
[ ca ]
default_ca = CA_default
[ CA_default ]
dir = $pki_dir/root
database = \$dir/index.txt
new_certs_dir = \$dir/newcerts
certificate = \$dir/root.pem
private_key = \$dir/root.key
serial = \$dir/serial
crlnumber = \$dir/crlnumber
default_md = sha256
default_days = 3650
default_crl_days = 3650
policy = policy_any
unique_subject = no
[ policy_any ]
commonName = supplied
[ req ]
distinguished_name = dn
x509_extensions = root_ca
prompt = no
[ dn ]
CN = CoreLink Trusted-Time Fixture Root
[ root_ca ]
basicConstraints = critical, CA:true, pathlen:1
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid:always
[ intermediate_ca ]
basicConstraints = critical, CA:true, pathlen:0
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid:always,issuer
CONF
cat > "$pki_dir/intermediate/openssl.cnf" <<CONF
[ ca ]
default_ca = CA_default
[ CA_default ]
dir = $pki_dir/intermediate
database = \$dir/index.txt
new_certs_dir = \$dir/newcerts
certificate = \$dir/intermediate.pem
private_key = \$dir/intermediate.key
serial = \$dir/serial
crlnumber = \$dir/crlnumber
default_md = sha256
default_days = 3650
default_crl_days = 3650
policy = policy_any
unique_subject = no
[ policy_any ]
commonName = supplied
[ tsa_good ]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, timeStamping
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid,issuer
[ tsa_bad_eku ]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = serverAuth
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid,issuer
CONF
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$pki_dir/root/root.key" -out "$pki_dir/root/root.pem" -config "$pki_dir/root/openssl.cnf" -days 3650 >/dev/null 2>&1
openssl req -new -newkey rsa:2048 -nodes -keyout "$pki_dir/intermediate/intermediate.key" -out "$pki_dir/intermediate/intermediate.csr" -subj '/CN=CoreLink Trusted-Time Fixture Intermediate' >/dev/null 2>&1
openssl ca -batch -config "$pki_dir/root/openssl.cnf" -extensions intermediate_ca -in "$pki_dir/intermediate/intermediate.csr" -out "$pki_dir/intermediate/intermediate.pem" >/dev/null 2>&1
openssl x509 -in "$pki_dir/intermediate/intermediate.pem" -out "$pki_dir/intermediate/intermediate.clean.pem"
mv "$pki_dir/intermediate/intermediate.clean.pem" "$pki_dir/intermediate/intermediate.pem"
openssl req -new -newkey rsa:2048 -nodes -keyout "$pki_dir/tsa-good.key" -out "$pki_dir/tsa-good.csr" -subj '/CN=CoreLink Fixture TSA' >/dev/null 2>&1
openssl ca -batch -config "$pki_dir/intermediate/openssl.cnf" -extensions tsa_good -in "$pki_dir/tsa-good.csr" -out "$pki_dir/tsa-good.pem" >/dev/null 2>&1
openssl x509 -in "$pki_dir/tsa-good.pem" -out "$pki_dir/tsa-good.clean.pem"
mv "$pki_dir/tsa-good.clean.pem" "$pki_dir/tsa-good.pem"
openssl req -new -newkey rsa:2048 -nodes -keyout "$pki_dir/tsa-bad-eku.key" -out "$pki_dir/tsa-bad-eku.csr" -subj '/CN=CoreLink Fixture Wrong EKU TSA' >/dev/null 2>&1
openssl ca -batch -config "$pki_dir/intermediate/openssl.cnf" -extensions tsa_bad_eku -in "$pki_dir/tsa-bad-eku.csr" -out "$pki_dir/tsa-bad-eku.pem" >/dev/null 2>&1
openssl x509 -in "$pki_dir/tsa-bad-eku.pem" -out "$pki_dir/tsa-bad-eku.clean.pem"
mv "$pki_dir/tsa-bad-eku.clean.pem" "$pki_dir/tsa-bad-eku.pem"
openssl ca -gencrl -config "$pki_dir/intermediate/openssl.cnf" -out "$pki_dir/intermediate/leaf-valid.crl.pem" >/dev/null 2>&1
openssl ca -gencrl -config "$pki_dir/root/openssl.cnf" -out "$pki_dir/root/intermediate-valid.crl.pem" >/dev/null 2>&1
openssl ca -gencrl -config "$pki_dir/intermediate/openssl.cnf" -crl_lastupdate 20200101000000Z -crl_nextupdate 20200102000000Z -out "$pki_dir/intermediate/leaf-stale.crl.pem" >/dev/null 2>&1
openssl ca -config "$pki_dir/intermediate/openssl.cnf" -revoke "$pki_dir/tsa-good.pem" >/dev/null 2>&1
openssl ca -gencrl -config "$pki_dir/intermediate/openssl.cnf" -out "$pki_dir/intermediate/leaf-revoked.crl.pem" >/dev/null 2>&1
openssl ca -config "$pki_dir/root/openssl.cnf" -revoke "$pki_dir/intermediate/intermediate.pem" >/dev/null 2>&1
openssl ca -gencrl -config "$pki_dir/root/openssl.cnf" -out "$pki_dir/root/intermediate-revoked.crl.pem" >/dev/null 2>&1
rm -f "$pki_dir"/*.csr "$pki_dir/root/root.key" "$pki_dir/root/index.txt"* "$pki_dir/root/serial" "$pki_dir/root/crlnumber" "$pki_dir/root/crlnumber.old" "$pki_dir/root/serial.old" "$pki_dir/root/openssl.cnf" "$pki_dir/root/newcerts"/* "$pki_dir/intermediate/intermediate.key" "$pki_dir/intermediate/intermediate.csr" "$pki_dir/intermediate/index.txt"* "$pki_dir/intermediate/serial" "$pki_dir/intermediate/crlnumber" "$pki_dir/intermediate/crlnumber.old" "$pki_dir/intermediate/serial.old" "$pki_dir/intermediate/openssl.cnf" "$pki_dir/intermediate/newcerts"/*
