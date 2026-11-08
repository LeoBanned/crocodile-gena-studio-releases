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
    const page=await browser.newPage({viewport:{width:1440,height:1000}});
    const errors=[];page.on('pageerror',e=>errors.push(e.message));
    await page.route('https://**/*',r=>r.abort());
    await page.addInitScript(()=>{
      localStorage.setItem('cgs-language','en');
      window.calls=[];window.pick=null;
      window.cloud={enabled:false,managed:true,folder:null,eligibleApps:['10'],loaderSupported:true,componentUpdateAvailable:true,issue:null};
      window.__TAURI__={event:{listen:async()=>()=>{}},core:{invoke:async(name,args)=>{
        window.calls.push({name,args});
        if(name==='local_cloud_status')return structuredClone(window.cloud);
        if(name==='local_cloud_update_component'){
          if(window.pauseCloudUpdate) await new Promise(resolve=>window.resumeCloudUpdate=resolve);
          if(window.cloudError)throw new Error(window.cloudError);
          window.cloud={...window.cloud,componentUpdateAvailable:false};
          return structuredClone(window.cloud);
        }
        if(name==='local_cloud_set'){
          if(window.cloudError)throw new Error(window.cloudError);
          window.cloud={...window.cloud,enabled:args.enabled,folder:args.folder,managed:args.enabled};
          return structuredClone(window.cloud);
        }
        if(name==='pick_game_directory')return window.pick;
        if(name==='platform_info')return{os:'windows',windows:true,windowsOnlyModules:[]};
        if(name==='check_for_update')return{available:false,currentVersion:'5.6.3',latestVersion:'5.6.3'};
        if(name==='list_games')return{games:[],selected_id:null};
        if(name==='get_app_status')return{games_count:0,admin:false,game_running:false};
        if(name==='is_selected_game_running')return false;
        if(name==='luatools_status')return{steamPath:'C:\\Steam',games:[],loaderStatus:'ready'};
        if(name==='luatools_check_loader_updates')return{updateAvailable:false,selectedVariant:'standard',variants:[]};
        if(name==='millennium_plugin_status')return{status:'ready',pluginInstalled:true,millenniumInstalled:true};
        if(name==='files_catalog')return{files:[],cached:false};
        if(name==='scan_installed_steam_games')return{games:[],libraries:[],warnings:[]};
        return{};
      }}};
    });
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    const notice=page.locator('#lc-update-dialog');
    await notice.waitFor({state:'visible'});
    assert.match(await notice.textContent(),/Local Cloud DLL update required/);
    await page.evaluate(()=>window.cgsI18n.setLanguage('ru'));
    assert.match(await page.locator('#lc-update-title').textContent(),/Нужно обновить DLL/);
    await page.evaluate(()=>window.cgsI18n.setLanguage('en'));
    const dialogThemes=[];
    for(const theme of ['graphite','white','ocean']) {
      await page.evaluate(theme=>window.cgsPersonalization.set({theme}),theme);
      dialogThemes.push(await notice.evaluate(e=>getComputedStyle(e).background));
    }
    assert.notEqual(dialogThemes[0],dialogThemes[1]);
    await page.evaluate(()=>window.cgsPersonalization.set({theme:'graphite'}));
    await notice.screenshot({path:path.join(process.env.CGS_TEST_OUTPUT||require('node:os').tmpdir(),'cgs-cloud-dll-update-en.png')});
    await page.keyboard.press('Tab');
    assert.equal(await page.evaluate(()=>document.getElementById('lc-update-dialog').contains(document.activeElement)),true);
    assert.equal(await page.evaluate(()=>calls.filter(c=>c.name==='local_cloud_update_component').length),0);
    await page.keyboard.press('Escape');
    await notice.waitFor({state:'hidden'});
    await page.locator('.nav-item[data-view="lua"]').click();
    await page.waitForFunction(()=>!document.getElementById('lc-enabled').disabled);
    assert.equal(await notice.isVisible(),false); // dismissal lasts for this launch
    await page.locator('#lc-refresh').click();
    await notice.waitFor({state:'visible'});
    await page.evaluate(()=>window.cloudError='Test: close the game first');
    await page.locator('#lc-update-install').click();
    await page.waitForFunction(()=>document.getElementById('lc-update-status').textContent.includes('Test:'));
    assert.equal(await notice.isVisible(),true); // can retry after a failure
    await page.evaluate(()=>{window.cloudError=null;window.pauseCloudUpdate=true;});
    await page.locator('#lc-update-install').click();
    await page.waitForFunction(()=>typeof window.resumeCloudUpdate==='function');
    assert.equal(await page.locator('#lc-update-later').isDisabled(),true);
    await page.keyboard.press('Escape');
    assert.equal(await notice.isVisible(),true); // cannot dismiss mid replacement
    await page.evaluate(()=>window.resumeCloudUpdate());
    await page.waitForFunction(()=>document.getElementById('lc-update-status').textContent.includes('DLL updated'));
    assert.equal(await page.locator('#lc-enabled').isChecked(),false); // update never enables cloud
    assert.equal(await page.evaluate(()=>calls.filter(c=>c.name==='local_cloud_set').length),0);
    await page.locator('#lc-update-later').click();
    await page.locator('#lc-refresh').click();
    await page.waitForFunction(()=>!document.getElementById('lc-refresh').disabled);
    assert.equal(await notice.isVisible(),false); // current DLL: no notice
    await page.locator('.local-cloud-toggle .switch').click(); // cancelled folder picker
    await page.waitForFunction(()=>!document.getElementById('lc-enabled').checked);
    assert.equal(await page.evaluate(()=>calls.filter(c=>c.name==='local_cloud_set').length),0);
    await page.evaluate(()=>window.pick='D:\\Local saves');
    await page.locator('#lc-browse').click();
    assert.equal(await page.locator('#lc-folder').inputValue(),'D:\\Local saves');
    await page.locator('.local-cloud-toggle .switch').click();
    await page.waitForFunction(()=>document.getElementById('lc-enabled').checked);
    assert.equal(await page.evaluate(()=>calls.filter(c=>c.name==='local_cloud_set').at(-1).args.enabled),true);
    await page.evaluate(()=>window.cloudError='Test: Steam is still running');
    await page.locator('.local-cloud-toggle .switch').click();
    await page.waitForFunction(()=>document.getElementById('lc-status').textContent.includes('Test:'));
    assert.equal(await page.locator('#lc-enabled').isChecked(),true);
    await page.evaluate(()=>window.cloudError=null);
    await page.locator('.local-cloud-toggle .switch').click();
    await page.waitForFunction(()=>!document.getElementById('lc-enabled').checked);
    assert.equal(await page.locator('#lc-folder').inputValue(),'D:\\Local saves');
    const backgrounds=[];
    for(const theme of ['graphite','white','ocean']){
      await page.evaluate(theme=>window.cgsPersonalization.set({theme}),theme);
      backgrounds.push(await page.locator('#local-cloud-panel').evaluate(e=>getComputedStyle(e).background));
    }
    assert.notEqual(backgrounds[0],backgrounds[1]);
    await page.evaluate(()=>window.cgsPersonalization.set({theme:'graphite'}));
    await page.locator('#local-cloud-panel').screenshot({path:path.join(process.env.CGS_TEST_OUTPUT||require('node:os').tmpdir(),'cgs-local-cloud-en.png')});
    await page.evaluate(()=>window.cgsI18n.setLanguage('ru'));
    assert.equal(await page.locator('#lc-title').textContent(),'Локальное облако');
    assert.equal(errors.length,0,errors.join('\n'));
    await page.reload();
    await notice.waitFor({state:'visible'}); // later means remind again on next launch
    console.log('Local cloud UI: startup DLL notice, dismissal, manual reopen, failure/retry, busy Escape protection, update without enabling, current DLL silence; toggle/folder/themes/language passed. Mock IPC only.');
  } finally {await browser.close();await new Promise(r=>server.close(r));}
})().catch(e=>{console.error(e);process.exitCode=1;});
