/* ns_gaps.c: settles fp_nquery headings under a non-zero pfcode, and MINFO/RP with single-label names. Offline: no network. */
#include <stdio.h>
#include <string.h>
#include <resolv.h>
#include <arpa/nameser.h>
static unsigned char m[512]; static int n;
static void put16(unsigned v){m[n++]=v>>8;m[n++]=v;}
static void put32(unsigned v){put16(v>>16);put16(v&0xffff);}
static void name(const char*s){ /* s: dotted, no trailing dot; "" = root */
  while(*s){const char*d=strchr(s,'.');int l=d?(int)(d-s):(int)strlen(s);m[n++]=l;memcpy(m+n,s,l);n+=l;s+=l;if(d)s++;}
  m[n++]=0;}
static void rr(const char*o,int t,const char*a,const char*b){
  name(o);put16(t);put16(1);put32(300);int at=n;n+=2;name(a);if(b)name(b);int l=n-at-2;m[at]=l>>8;m[at+1]=l;}
static void msg(int an){
  n=0;put16(0x1234);put16(0x8580);put16(1);put16(an);put16(0);put16(0);
  name("a.test");put16(14);put16(1);
  rr("a.test",14,"r","e");        /* MINFO single-label */
  rr("a.test",17,"mbox","txt");   /* RP single-label */
  rr("a.test",14,"","");          /* MINFO root names */
  rr("a.test",17,"r.b.test","t.test"); }
int main(void){
  unsigned long pf[]={0,0x10,0x20,0x100,0x110,0x130,0x1f0,0x300,0x800,0x8f0,0x9f0,0xb30,0x40,0x80,0x140,0x180};
  res_init();
  for(unsigned i=0;i<sizeof pf/sizeof*pf;i++){
    msg(4);_res.pfcode=pf[i];printf("== pfcode %#lx\n",pf[i]);fflush(stdout);
    fp_nquery(m,n,stdout);fflush(stdout);}
  printf("== ns_sprintrr\n");
  msg(4);ns_msg h;ns_initparse(m,n,&h);
  for(int i=0;i<4;i++){ns_rr r;ns_parserr(&h,ns_s_an,i,&r);char b[1024];
    int k=ns_sprintrr(&h,&r,NULL,NULL,b,sizeof b);printf("%d [%s]\n",k,b);}
  return 0;}
