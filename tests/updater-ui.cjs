const assert=require('node:assert/strict');
const fs=require('node:fs/promises');
const path=require('node:path');
const http=require('node:http');
const {chromium}=require('playwright');
(async()=>{
 const root=path.resolve(__dirname,'../ui');
 const server=http.createServer(async(req,res)=>{
  const file=path.resolve(root,decodeURIComponent(req.url.split('?')[0]).replace(/^\/+/, '')||'index.html');
  if(!file.startsWith(root+path.sep))return res.writeHead(403).end();
  try{res.setHeader('Content-Type',{'.html':'text/html','.css':'text/css','.js':'text/javascript','.png':'image/png'}[path.extname(file)]||'application/octet-stream');res.end(await fs.readFile(file));}catch{res.writeHead(404).end();}
 });
 await new Promise(r=>server.listen(0,'127.0.0.1',r));
 const browser=await chromium.launch({executablePath:process.env.CGS_BROWSER||'C:/Program Files (x86)/Microsoft/EdgeCore/156.0.4314.58/msedge.exe',headless:true});
 try{
  const page=await browser.newPage({viewport:{width:1440,height:1000}});const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.route('https://**/*',r=>r.abort());
  await page.addInitScript(()=>{
   localStorage.setItem('cgs-language','en');window.calls=[];window.events={};window.available=localStorage.getItem('updater-test-available')==='yes'; const nativeTimeout=window.setTimeout.bind(window); window.setTimeout=(fn,ms,...args)=>nativeTimeout(fn,ms===1800?20:ms,...args);
   window.__TAURI__={event:{listen:async(name,fn)=>{window.events[name]=fn;return()=>{}}},core:{invoke:async(name,args)=>{
    calls.push({name,args});
    if(name==='install_update')return new Promise((resolve,reject)=>{window.rejectInstall=reject});
    if(name==='cancel_update'){window.rejectInstall(new Error('Cancelled by test'));return null;}
    if(name==='check_for_update')return{available:window.available,currentVersion:'5.6.3',latestVersion:'5.6.4',inAppSupported:true,notes:'Test release'};
    if(name==='update_launch_ready'){window.startupReady=true;return null;}
    if(name==='platform_info')return{os:'windows',windows:true,windowsOnlyModules:[],sessionId:localStorage.getItem('updater-test-session') || 'first-native-process'};
    if(name==='list_games')return{games:[],selected_id:null};
    if(name==='get_app_status')return{games_count:0,admin:false,game_running:false};
    if(name==='is_selected_game_running')return false;
    if(name==='files_catalog')return{files:[],cached:false};
    if(name==='scan_installed_steam_games')return{games:[],libraries:[],warnings:[]};
    return{};
   }}};
  });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.locator('.nav-item[data-view="settings"]').click();
  await page.waitForFunction(()=>window.events['cgs-update-progress']);
  await page.waitForTimeout(80);
  await page.evaluate(()=>window.available=true);
  await page.locator('#check-update-button').click();
  await page.locator('#app-update-backdrop').waitFor({state:'visible'});
  assert.equal(await page.locator('#check-update-button').isDisabled(),false);
  await page.evaluate(()=>window.dispatchEvent(new CustomEvent('cgs:busy-changed',{detail:{id:'archive',busy:true}})));
  await page.locator('#app-update-install').click();
  assert.equal(await page.evaluate(()=>calls.filter(c=>c.name==='install_update').length),0);
  await page.evaluate(()=>window.dispatchEvent(new CustomEvent('cgs:busy-changed',{detail:{id:'archive',busy:false}})));
  await page.locator('#app-update-install').click();
  await page.waitForFunction(()=>window.rejectInstall);
  await page.evaluate(()=>window.events['cgs-update-progress']({payload:{phase:'downloading',downloaded:0,total:50*1048576,percent:0}}));
  await page.waitForTimeout(150);
  await page.evaluate(()=>window.events['cgs-update-progress']({payload:{phase:'downloading',downloaded:25*1048576,total:50*1048576,percent:50}}));
  assert.equal(await page.locator('#app-update-progress').getAttribute('value'),'50');
  assert.match(await page.locator('#app-update-detail').textContent(),/MB\/s/);
  assert.equal(await page.locator('#app-update-backdrop').isVisible(),true);
  assert.equal(await page.locator('.app-shell').evaluate(e=>e.inert),true);
  assert.equal(await page.locator('#check-update-button').isDisabled(),true);
  if(process.env.CGS_TEST_OUTPUT)await page.locator('.app-update-dialog').screenshot({path:path.join(process.env.CGS_TEST_OUTPUT,'updater-dialog-en.png')});
  await page.locator('#app-update-cancel').click();
  await page.locator('#app-update-install').waitFor({state:'visible'});
  assert.match(await page.locator('#app-update-phase').textContent(),/cancelled/i);
  assert.match(await page.locator('#app-update-detail').textContent(),/Cancelled by test/);
  await page.locator('#app-update-install').click();
  await page.waitForFunction(()=>document.getElementById('check-update-button').disabled);
  await page.evaluate(()=>window.events['cgs-update-progress']({payload:{phase:'installing',downloaded:50*1048576,total:50*1048576,percent:100}}));
  assert.equal(await page.locator('#app-update-cancel').isDisabled(),false);
  await page.locator('#app-update-cancel').click();
  await page.locator('#app-update-install').waitFor({state:'visible'});
  await page.locator('#app-update-install').click();
  await page.waitForFunction(()=>document.getElementById('check-update-button').disabled);
  await page.evaluate(()=>window.rejectInstall(new Error('SHA-256 mismatch')));
  await page.locator('#app-update-install').waitFor({state:'visible'});
  assert.match(await page.locator('#app-update-detail').textContent(),/SHA-256 mismatch/);
  assert.equal(await page.locator('#update-current-version').textContent(),'5.6.3');
  let calls=await page.evaluate(()=>window.calls);
  assert.equal(calls.filter(c=>c.name==='install_update').length,3);
  assert.equal(calls.filter(c=>c.name==='install_update').at(-1).args.version,'5.6.4');
  assert.equal(calls.filter(c=>c.name==='open_external').length,0);
  await page.evaluate(()=>localStorage.setItem('updater-test-available','yes'));
  await page.locator('#app-update-defer').click();
  assert.equal(await page.evaluate(()=>localStorage.getItem('cgs-update-snooze-launches')),'5');
  assert.equal(await page.locator('.app-shell').evaluate(e=>e.inert),false);
  await page.reload();await page.waitForFunction(()=>window.startupReady);await page.waitForTimeout(80);
  assert.equal(await page.evaluate(()=>localStorage.getItem('cgs-update-snooze-launches')),'5','A WebView reload must not consume another launch');
  assert.equal(await page.locator('#app-update-backdrop').isVisible(),false);
  for(let launch=1;launch<=5;launch++){
    await page.evaluate(id=>localStorage.setItem('updater-test-session',id),`skipped-native-process-${launch}`);
    await page.reload();await page.waitForFunction(()=>window.startupReady);await page.waitForTimeout(80);
    assert.equal(await page.locator('#app-update-backdrop').isVisible(),false,`Unexpected reminder on skipped launch ${launch}`);
    assert.equal(await page.evaluate(()=>calls.filter(c=>c.name==='check_for_update').length),0);
    assert.equal(await page.evaluate(()=>localStorage.getItem('cgs-update-snooze-launches')),String(5-launch));
    if(launch===1){
      await page.locator('.nav-item[data-view="settings"]').click();
      await page.locator('#check-update-button').click();
      await page.locator('#app-update-backdrop').waitFor({state:'visible'});
      assert.equal(await page.evaluate(()=>localStorage.getItem('cgs-update-snooze-launches')),'4');
      await page.locator('#app-update-close').click();
    }
  }
  await page.evaluate(()=>localStorage.setItem('updater-test-session','sixth-native-process'));
  await page.reload();await page.waitForFunction(()=>window.startupReady);
  await page.locator('#app-update-backdrop').waitFor({state:'visible'});
  assert.equal(await page.evaluate(()=>calls.filter(c=>c.name==='check_for_update').length),1);
  await page.locator('#app-update-defer').click();
  assert.equal(await page.evaluate(()=>localStorage.getItem('cgs-update-snooze-launches')),'5');
  assert.deepEqual(errors,[]);
  console.log('PASS updater: persistent modal, speed/progress, download and installation cancellation, retry, five skipped launches, sixth reminder, manual check during snooze, repeatable deferral, no browser.');
 }finally{await browser.close();await new Promise(r=>server.close(r));}
})().catch(e=>{console.error(e);process.exitCode=1});
