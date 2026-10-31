#!/usr/bin/env node
import { listUsers } from '../src/users.js';
for (const u of listUsers()) console.log(`${u.id}\t${u.first} ${u.last}\t${u.created}`);
