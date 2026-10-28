import { execFileSync } from 'node:child_process';
import { existsSync, writeFileSync, renameSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
const localCargo=join(process.env.CARGO_HOME??join(homedir(),'.cargo'),'bin/cargo');
const cargo=existsSync(localCargo)?localCargo:'cargo';
// A failed generator must never truncate the previously generated module.
const generated=execFileSync(cargo,['run','--locked','-q','-p','rdm-protocol','--example','export'],{encoding:'utf8',stdio:['ignore','pipe','inherit']});
if(!generated.includes('export type Snapshot'))throw new Error('Invalid generated protocol module');
if(process.argv.includes('--check')) {
  if(readFileSync('src/protocol.ts','utf8')!==generated)throw new Error('Generated protocol DTOs differ. Run npm run types and commit the result.');
  console.log('Generated TypeScript DTOs match Rust protocol.');
} else {
  writeFileSync('src/protocol.ts.tmp',generated);renameSync('src/protocol.ts.tmp','src/protocol.ts');
}
