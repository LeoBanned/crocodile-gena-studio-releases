// Isolated updater protocol fixture: never opens Steam, games or user settings.
#include <windows.h>
#include <string>
static bool exists(const wchar_t* path){return GetFileAttributesW(path)!=INVALID_FILE_ATTRIBUTES;}
static bool write(const std::wstring& path,const std::string& value){
    HANDLE file=CreateFileW(path.c_str(),GENERIC_WRITE,FILE_SHARE_READ,nullptr,CREATE_NEW,FILE_ATTRIBUTE_NORMAL,nullptr);
    if(file==INVALID_HANDLE_VALUE)return false;DWORD count=0;
    bool ok=WriteFile(file,value.data(),DWORD(value.size()),&count,nullptr)&&count==value.size();FlushFileBuffers(file);CloseHandle(file);return ok;
}
int wmain(int argc,wchar_t** argv){
    HANDLE helperLock=INVALID_HANDLE_VALUE;
    if(argc==3 && std::wstring(argv[1])==L"--hold-helper"){
        helperLock=CreateFileW(argv[2],GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,FILE_ATTRIBUTE_NORMAL,nullptr);
        if(helperLock==INVALID_HANDLE_VALUE)return 21;
    }
    if(argc==4 && std::wstring(argv[1])==L"--cgs-update-probe"){
        if(exists(L"fail-new"))return 17;
        std::wstring nonce(argv[3]);std::string ascii;for(wchar_t c:nonce)ascii+=static_cast<char>(c);
        if(!write(std::wstring(argv[2])+L"\\healthy",ascii))return 18;
        write(L"new-started","yes");Sleep(1000);return 0;
    }
    if(argc==4 && std::wstring(argv[1])==L"--cgs-update-failed"){write(L"rollback-restarted","yes");return 0;}
    for(int i=0;i<600;i++){if(exists(L"exit-parent")){if(helperLock!=INVALID_HANDLE_VALUE)CloseHandle(helperLock);return 0;}Sleep(100);}
    return 19;
}
