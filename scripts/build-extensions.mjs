import{build}from'esbuild';import{mkdir,readFile,writeFile,copyFile,rm}from'node:fs/promises';
const identity=JSON.parse(await readFile('extensions/identity.json','utf8'));
for(const target of ['chromium','firefox']){
 const out=`extensions/dist/${target}`;await rm(out,{recursive:true,force:true});await mkdir(out,{recursive:true});
 await build({entryPoints:['extensions/src/background.ts','extensions/src/popup.ts'],bundle:true,outdir:out,format:'esm',target:'es2022',minify:true});
 await build({entryPoints:['extensions/src/content.ts'],bundle:true,outdir:out,format:'iife',target:'es2022',minify:true});
 for(const file of ['popup.html','popup.css'])await copyFile(`extensions/src/${file}`,`${out}/${file}`);
 await copyFile('src-tauri/icons/128x128.png',`${out}/icon.png`);
 const manifest={manifest_version:3,name:'Rust Download Manager',version:'0.2.2',description:'Send HTTP and HTTPS downloads to your local Rust Download Manager.',permissions:['nativeMessaging','contextMenus','downloads','storage','activeTab','webRequest'],optional_permissions:['cookies'],optional_host_permissions:['http://*/*','https://*/*'],action:{default_popup:'popup.html',default_icon:'icon.png'},icons:{128:'icon.png'}};
 manifest.content_scripts=[{matches:['http://*/*','https://*/*'],js:['content.js'],all_frames:true,run_at:'document_idle'}];
 if(target==='chromium'){manifest.key=identity.key;manifest.background={service_worker:'background.js',type:'module'};}
 else{manifest.background={scripts:['background.js'],type:'module'};manifest.browser_specific_settings={gecko:{id:'rdm@download-manager.local',strict_min_version:'140.0',data_collection_permissions:{required:['none']}}};}
 await writeFile(`${out}/manifest.json`,JSON.stringify(manifest,null,2)+'\n');
}
console.log('Built extensions/dist/chromium and extensions/dist/firefox');
