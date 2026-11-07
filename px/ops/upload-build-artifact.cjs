// Transfer a build artifact through the existing authenticated multipart API.
// Credentials and temporary upload tokens are never logged.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
async function main() {
  const [filename, record, accessFilename] = process.argv.slice(2);
  if (!filename || !record || !accessFilename) throw new Error('Usage: node upload-build-artifact.cjs <file> <private-receipt> <private-access.json>');
  const privateRoot = path.dirname(path.resolve(accessFilename));
  const output = path.resolve(record);
  if (!output.startsWith(privateRoot + path.sep)) throw new Error('Receipt must stay private');
  const access = JSON.parse(fs.readFileSync(path.resolve(accessFilename)));
  const group = access.groups.find(g => g.name === 'GPT-Private' && g.enabled);
  if (!group) throw new Error('Enabled GPT-Private key is required');
  const base = 'https://masub-ex.yuhatze.org/comments/artifacts/uploads';
  const bytes = fs.readFileSync(filename);
  const sha256 = crypto.createHash('sha256').update(bytes).digest('hex');
  const headers = {Authorization: `Bearer ${group.api_key}`};
  async function call(url, method, body, extra = {}) {
    const response = await fetch(url, {method, body, headers: {...headers, ...extra}, signal: AbortSignal.timeout(120000)});
    if (!response.ok) throw new Error(`Artifact transfer HTTP ${response.status}`);
    return response.json();
  }
  let state = fs.existsSync(output) ? JSON.parse(fs.readFileSync(output)) : null;
  if (state && state.sha256 !== sha256) throw new Error('Receipt belongs to a different artifact');
  if (!state) {
    const session = await call(base, 'POST', JSON.stringify({mime_type:'application/octet-stream', size:bytes.length}), {'Content-Type':'application/json'});
    state = {sha256, size: bytes.length, session, parts: []};
    fs.writeFileSync(output, JSON.stringify(state));
  }
  const chunkSize = 2 * 1024 * 1024;
  for (let offset = 0, part = 1; offset < bytes.length; offset += chunkSize, part++) {
    if (state.parts.some(p => p.part_number === part)) continue;
    const result = await call(`${base}/${state.session.upload_id}/parts/${part}`, 'PUT', bytes.subarray(offset, Math.min(offset + chunkSize, bytes.length)), {'Content-Type':'application/octet-stream', 'x-upload-token':state.session.upload_token});
    state.parts.push(result);
    fs.writeFileSync(output, JSON.stringify(state));
    console.log(JSON.stringify({part, uploaded:Math.min(offset+chunkSize,bytes.length), total:bytes.length}));
  }
  if (!state.completed) {
    state.completed = await call(`${base}/${state.session.upload_id}/complete`, 'POST', JSON.stringify({upload_token:state.session.upload_token, parts:state.parts}), {'Content-Type':'application/json'});
    fs.writeFileSync(output, JSON.stringify(state));
  }
  console.log(JSON.stringify({completed:true, size:bytes.length, sha256, artifact:state.completed.path}));
}
main().catch(error => { console.error(error.message); process.exitCode = 1; });
