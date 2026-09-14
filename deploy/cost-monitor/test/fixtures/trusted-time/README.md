# Offline RFC 3161 fixture PKI

`generate-fixtures.sh` rebuilds this test-only RSA PKI with OpenSSL. It contains a root, an intermediate, a valid TSA leaf with critical `timeStamping` EKU, an intentionally wrong-EKU leaf, and valid, stale, and revoking CRLs. The two private keys are synthetic fixture material only; they do not anchor or configure production trust.

Tests generate a fresh RFC 3161 reply from each inbound query, so the signed reply is bound to the query's random nonce and imprint. No test contacts a network authority.
