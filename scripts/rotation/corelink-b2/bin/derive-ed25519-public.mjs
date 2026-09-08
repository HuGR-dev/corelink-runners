#!/usr/bin/env node
import { createHash, createPrivateKey, createPublicKey } from 'node:crypto';
import { readFileSync } from 'node:fs';

const args = process.argv.slice(2);
if (args.length !== 2 || args[0] !== '--seed-file') throw new Error('usage: --seed-file FILE');
const text = readFileSync(args[1], 'utf8').trim();
if (!/^[A-Za-z0-9+/]{43}=$/.test(text)) throw new Error('seed must be standard-base64 32 bytes');
const seed = Buffer.from(text, 'base64');
if (seed.length !== 32 || seed.toString('base64') !== text) throw new Error('invalid seed');
const privateDer = Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), seed]);
const privateKey = createPrivateKey({ key: privateDer, format: 'der', type: 'pkcs8' });
const publicDer = createPublicKey(privateKey).export({ format: 'der', type: 'spki' });
const prefix = Buffer.from('302a300506032b6570032100', 'hex');
if (publicDer.length !== prefix.length + 32 || !publicDer.subarray(0, prefix.length).equals(prefix)) throw new Error('unexpected Ed25519 SPKI');
const pub = publicDer.subarray(prefix.length);
process.stdout.write(`key_id=${createHash('sha256').update(pub).digest('hex').slice(0, 16)}\npubkey_b64=${pub.toString('base64')}\n`);
