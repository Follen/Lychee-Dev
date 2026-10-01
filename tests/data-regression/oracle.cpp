// Test-only adapter for CascLib. No upstream implementation is vendored here.
#include "CascLib.h"
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>

static int fail(const char *stage) { std::cerr << stage << ": " << GetCascError() << "\n"; return 1; }
static std::string hex(const BYTE *raw) { const char *digits="0123456789abcdef"; std::string out; for(int i=0;i<16;i++){out+=digits[raw[i]>>4];out+=digits[raw[i]&15];} return out; }

int main(int argc,char **argv) {
 HANDLE storage=NULL,file=NULL;
 const char *output=NULL; unsigned long long maximum=0;
 CASC_FILE_FULL_INFO info={}; CASC_STORAGE_PRODUCT product={};
 std::string mode=argc>1?argv[1]:"";
 if(mode=="raw" && argc==5) {
  output=argv[3]; maximum=std::strtoull(argv[4],NULL,10);
  if(!CascOpenLocalFile(argv[2],CASC_STRICT_DATA_CHECK,&file))return fail("open raw BLTE");
 } else if(mode=="storage" && argc==12) {
  output=argv[10];maximum=std::strtoull(argv[11],NULL,10);
  CASC_OPEN_STORAGE_ARGS args={};args.Size=sizeof(args);args.szLocalPath=argv[2];args.szCodeName=argv[3];args.szBuildKey=argv[4];args.dwLocaleMask=std::strtoul(argv[7],NULL,0);
  if(!CascOpenStorageEx(NULL,&args,false,&storage))return fail("open pinned offline storage");
  if(!CascGetStorageInfo(storage,CascStorageProduct,&product,sizeof(product),NULL) || product.BuildNumber!=std::strtoul(argv[5],NULL,10) || std::strcmp(product.szCodeName,argv[3]))return fail("storage product/build identity");
  if(std::strcmp(argv[9],"-") && !CascImportKeysFromFile(storage,argv[9]))return fail("import explicit keys");
  DWORD id=std::strtoul(argv[6],NULL,10);
  if(!CascOpenFile(storage,CASC_FILE_DATA_ID(id),args.dwLocaleMask,CASC_OPEN_BY_FILEID|CASC_STRICT_DATA_CHECK,&file))return fail("open exact FDID");
  if(!CascGetFileInfo(file,CascFileFullInfo,&info,sizeof(info),NULL) || info.FileDataId!=id || !(info.LocaleFlags&args.dwLocaleMask))return fail("FDID/locale identity");
  bool low=(info.ContentFlags&CASC_CFLAG_LOW_VIOLENCE)!=0;
  if((std::strcmp(argv[8],"standard") && std::strcmp(argv[8],"low-violence")) || low!=(std::strcmp(argv[8],"low-violence")==0))return fail("explicit content variant");
 } else { std::cerr << "raw <BLTE> <output> <maxBytes> OR storage <installation> <product> <buildConfig> <buildNumber> <FDID> <localeMask> <standard|low-violence> <keys|-> <output> <maxBytes>\n";return 2; }
 if(maximum==0 || maximum>(512ULL<<20))return fail("byte bound");
 if(FILE *existing=std::fopen(output,"rb")){std::fclose(existing);return fail("output exists");}
 FILE *target=std::fopen(output,"wb");if(!target)return fail("create output");
 BYTE buffer[65536]; unsigned long long total=0;DWORD read=0;
 while(true){
  if(!CascReadFile(file,buffer,sizeof(buffer),&read)){std::fclose(target);std::remove(output);return fail("strict read (missing keys never zero-filled)");}
  if(read==0)break;
  if(total+read>maximum || std::fwrite(buffer,1,read,target)!=read){std::fclose(target);std::remove(output);return fail("bounded output");}
  total+=read;
 }
 if(std::fclose(target))return fail("flush output");
 CascCloseFile(file);if(storage)CascCloseStorage(storage);
 std::cout<<"{\"mode\":\""<<mode<<"\",\"bytes\":"<<total<<",\"contentKey\":\""<<(mode=="storage"?hex(info.CKey):"")<<"\",\"buildNumber\":"<<product.BuildNumber<<",\"fileDataID\":"<<info.FileDataId<<",\"localeMask\":"<<info.LocaleFlags<<",\"contentFlags\":"<<info.ContentFlags<<"}\n";
 return 0;
}
