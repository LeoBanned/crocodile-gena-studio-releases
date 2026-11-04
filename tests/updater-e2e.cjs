const fs=require('node:fs/promises');
const path=require('node:path');
const os=require('node:os');
const crypto=require('node:crypto');
const {spawn}=require('node:child_process');
const assert=require('node:assert/strict');
const root=path.resolve(__dirname,'..');
const fixtureDir=process.env.CGS_UPDATER_FIXTURES;
if(!fixtureDir)throw new Error('Build updater fixtures first; set CGS_UPDATER_FIXTURES to their directory.');
const hash=async file=>crypto.createHash('sha256').update(await fs.readFile(file)).digest('hex');
const delay=ms=>new Promise(r=>setTimeout(r,ms));
async function wait(file,timeout=15000){const end=Date.now()+timeout;while(Date.now()<end){try{return await fs.readFile(file,'utf8')}catch(e){if(e.code!=='ENOENT')throw e}await delay(50)}throw new Error('Timeout: '+file)}
async function run(mode){
 const dir=await fs.mkdtemp(path.join(os.tmpdir(),'cgs-updater-e2e-'));const nonce=crypto.randomBytes(16).toString('hex');
 const stage=path.join(dir,'cgs-update-'+nonce);await fs.mkdir(stage);
 const target=path.join(dir,'Программа с пробелами.exe');await fs.copyFile(path.join(fixtureDir,'old.exe'),target);
 await fs.copyFile(path.join(fixtureDir,'new.exe'),path.join(stage,'payload.exe'));
 await fs.copyFile(path.join(root,'src-tauri/resources/updater/CgsUpdater.exe'),path.join(stage,'updater.exe'));
 if(mode==='rollback')await fs.writeFile(path.join(dir,'fail-new'),'yes');
 const parent=spawn(target,['--hold-helper',path.join(stage,'updater.exe')],{cwd:dir,windowsHide:true});const exited=new Promise(r=>parent.once('exit',r));
 await new Promise((r,j)=>{parent.once('spawn',r);parent.once('error',j)});
 const p={schema:1,nonce,target,parent_pid:parent.pid,old_hash:await hash(target),new_hash:await hash(path.join(stage,'payload.exe')),version:'5.6.2'};
 await fs.writeFile(path.join(stage,'owner'),nonce);await fs.writeFile(path.join(stage,'plan.json'),JSON.stringify(p));
 const helper=spawn(path.join(stage,'updater.exe'),[stage,await hash(path.join(stage,'plan.json'))],{cwd:stage,windowsHide:true});const helperExited=new Promise(r=>helper.once('exit',r));
 try{
  await wait(path.join(stage,'ready'));assert.equal(await hash(target),p.old_hash);
  if(mode==='abort')await fs.writeFile(path.join(stage,'abort'),nonce);
  else{await fs.writeFile(path.join(stage,'go'),nonce);await fs.writeFile(path.join(dir,'exit-parent'),'yes');await exited;}
  const result=JSON.parse(await wait(path.join(stage,'result.json')));assert.equal(result.nonce,nonce);
  assert.equal(result.success,mode==='success');
  assert.equal(await hash(target),mode==='success'?p.new_hash:p.old_hash);
  if(mode==='success'){await wait(path.join(dir,'new-started'));assert.equal(await fs.stat(path.join(dir,`.cgs-update-${nonce}.old.exe`)).catch(()=>null),null);}
  if(mode==='rollback')await wait(path.join(dir,'rollback-restarted'));
  if(mode==='abort'){await fs.writeFile(path.join(dir,'exit-parent'),'yes');await exited;}
  await helperExited;
  console.log(`PASS native updater: ${mode}; Unicode/spaces path, actual helper process and atomic EXE replacement.`);
 }finally{
  if(parent.exitCode===null)parent.kill();if(helper.exitCode===null)helper.kill();
  await delay(1100);
  assert.equal(path.dirname(path.resolve(dir)),path.resolve(os.tmpdir()));
  assert.ok(path.basename(dir).startsWith('cgs-updater-e2e-'));
  await fs.rm(dir,{recursive:true,force:true});
 }
}
(async()=>{for(const mode of ['success','rollback','abort'])await run(mode)})().catch(e=>{console.error(e);process.exitCode=1});
