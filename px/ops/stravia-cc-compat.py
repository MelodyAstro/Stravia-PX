#!/usr/bin/env python3
"""Hot toggle for the managed Antigravity Claude compatibility component."""
import argparse,hashlib,importlib.util,json,pathlib
MANIFEST=pathlib.Path('/opt/stravia/compat/manifest.json')
def client():
 spec=importlib.util.spec_from_file_location('stravia_client','/root/stravia-client.py')
 module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
 return module.Client().login()
def main():
 parser=argparse.ArgumentParser();parser.add_argument('action',choices=['status','on','off','stock-rollback']);args=parser.parse_args()
 manifest=json.loads(MANIFEST.read_text());c=client()
 provider=next(p for p in c.call('GET','/api/v1/providers')['data'] if p['id']==manifest['provider_id'])
 plugin=next(p for p in c.call('GET','/api/v1/vendor-plugins')['data'] if p['vendor_id']=='antigravity')
 if args.action=='stock-rollback':
  import subprocess
  stock=pathlib.Path(manifest['stock_component'])
  assert hashlib.sha256(stock.read_bytes()).hexdigest()==manifest['stock_sha256'],'Stock component checksum mismatch'
  subprocess.run(['/usr/local/sbin/stravia-maint','backup'],check=True)
  options=dict(provider.get('vendor_options') or {})
  options.pop('claude_code_compat',None);options.pop('claude_code_compat_terms',None)
  c.call('PUT','/api/v1/providers/'+provider['id'],{'vendor_options':options})
  preview=c.call('POST','/api/v1/vendor-plugins/import',stock.read_bytes(),'application/wasm',timeout=600)['data']
  c.call('POST','/api/v1/vendor-plugins/confirm',{'preview_id':preview['id'],'allow_data_discard':False},timeout=600)
  MANIFEST.rename(MANIFEST.with_name('manifest.stock-restored.json'))
  print(json.dumps({'stock_restored':True,'version':manifest.get('stock_version','0.6.2')}));return
 assert plugin['version']==manifest['version'] and plugin['status']=='ready','Managed plugin is not active'
 if args.action in ['on','off']:
  options=dict(provider.get('vendor_options') or {});options['claude_code_compat']=args.action=='on'
  options.setdefault('claude_code_compat_terms','Anthropic')
  c.call('PUT','/api/v1/providers/'+provider['id'],{'vendor_options':options})
  provider=next(p for p in c.call('GET','/api/v1/providers')['data'] if p['id']==manifest['provider_id'])
 options=provider.get('vendor_options') or {}
 print(json.dumps({'version':plugin['version'],'enabled':options.get('claude_code_compat',False),'terms':options.get('claude_code_compat_terms','Anthropic'),'scope':'Antigravity claude-* only','restart_required':False}))
if __name__=='__main__':main()
