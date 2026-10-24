// Specified in part: `chflags`, `fchflags` (§13.15, phase 5); the rest is phase 15.
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §29 (305 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
#![allow(non_snake_case)]
#![allow(missing_docs)]
#![allow(clippy::unwrap_used)] // the §29.1 stream tests unwrap their own fixtures

use liella_boundary::errno;
use liella_boundary::posix::s13_filesystem as fs;
use spectest::symbol;

const ENOSYS: i32 = 38;

symbol!(sym_advance, "advance", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_authdes_create, "authdes_create", "29", "2.2.5", r##""##);
symbol!(sym_authdes_getucred, "authdes_getucred", "29", "2.2.5", r##""##);
symbol!(sym_authdes_pk_create, "authdes_pk_create", "29", "2.2.5", r##""##);
symbol!(sym__authenticate, "_authenticate", "29", "2.2.5", r##""##);
symbol!(sym_authnone_create, "authnone_create", "29", "2.2.5", r##""##);
symbol!(sym_authunix_create, "authunix_create", "29", "2.2.5", r##""##);
symbol!(sym_authunix_create_default, "authunix_create_default", "29", "2.2.5", r##""##);
symbol!(sym_bindresvport, "bindresvport", "29", "2.2.5", r##""##);
symbol!(sym_callrpc, "callrpc", "29", "2.2.5", r##""##);
symbol!(sym_cbc_crypt, "cbc_crypt", "29", "2.2.5", r##""##);
#[test]
fn sym_chflags() {
    errno::set_raw(0);
    assert_eq!((fs::chflags(c"/tmp".as_ptr(), 0), errno::get()), (-1, ENOSYS), "always ENOSYS (§13.15)");
    assert_eq!((fs::chflags(core::ptr::null(), 1), errno::get()), (-1, ENOSYS), "whatever the arguments");
}
symbol!(sym_clnt_broadcast, "clnt_broadcast", "29", "2.2.5", r##""##);
symbol!(sym_clnt_create, "clnt_create", "29", "2.2.5", r##""##);
symbol!(sym_clnt_pcreateerror, "clnt_pcreateerror", "29", "2.2.5", r##""##);
symbol!(sym_clnt_perrno, "clnt_perrno", "29", "2.2.5", r##""##);
symbol!(sym_clnt_perror, "clnt_perror", "29", "2.2.5", r##""##);
symbol!(sym_clnt_spcreateerror, "clnt_spcreateerror", "29", "2.2.5", r##""##);
symbol!(sym_clnt_sperrno, "clnt_sperrno", "29", "2.2.5", r##""##);
symbol!(sym_clnt_sperror, "clnt_sperror", "29", "2.2.5", r##""##);
symbol!(sym_clntraw_create, "clntraw_create", "29", "2.2.5", r##""##);
symbol!(sym_clnttcp_create, "clnttcp_create", "29", "2.2.5", r##""##);
symbol!(sym_clntudp_bufcreate, "clntudp_bufcreate", "29", "2.2.5", r##""##);
symbol!(sym_clntudp_create, "clntudp_create", "29", "2.2.5", r##""##);
symbol!(sym_clntunix_create, "clntunix_create", "29", "2.2.5", r##""##);
symbol!(sym_des_setparity, "des_setparity", "29", "2.2.5", r##""##);
symbol!(sym_ecb_crypt, "ecb_crypt", "29", "2.2.5", r##""##);
symbol!(sym_fattach, "fattach", "29", "2.2.5", r##"(no installed header declares it)"##);
#[test]
fn sym_fchflags() {
    errno::set_raw(0);
    assert_eq!((fs::fchflags(0, 0), errno::get()), (-1, ENOSYS), "always ENOSYS (§13.15)");
}
symbol!(sym_fdetach, "fdetach", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym___free_fdresult, "__free_fdresult", "29", "2.2.5", r##""##);
symbol!(sym_get_myaddress, "get_myaddress", "29", "2.2.5", r##""##);
symbol!(sym_getmsg, "getmsg", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_getnetname, "getnetname", "29", "2.2.5", r##""##);
symbol!(sym_getpmsg, "getpmsg", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_getpublickey, "getpublickey", "29", "2.2.5", r##""##);
symbol!(sym_getrpcport, "getrpcport", "29", "2.2.5", r##""##);
symbol!(sym_getsecretkey, "getsecretkey", "29", "2.2.5", r##""##);
symbol!(sym_host2netname, "host2netname", "29", "2.2.5", r##""##);
symbol!(sym_isastream, "isastream", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_key_decryptsession, "key_decryptsession", "29", "2.2.5", r##""##);
symbol!(sym_key_decryptsession_pk, "key_decryptsession_pk", "29", "2.2.5", r##""##);
symbol!(sym___key_decryptsession_pk_LOCAL, "__key_decryptsession_pk_LOCAL", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_key_encryptsession, "key_encryptsession", "29", "2.2.5", r##""##);
symbol!(sym_key_encryptsession_pk, "key_encryptsession_pk", "29", "2.2.5", r##""##);
symbol!(sym___key_encryptsession_pk_LOCAL, "__key_encryptsession_pk_LOCAL", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_key_gendes, "key_gendes", "29", "2.2.5", r##""##);
symbol!(sym___key_gendes_LOCAL, "__key_gendes_LOCAL", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_key_get_conv, "key_get_conv", "29", "2.2.5", r##""##);
symbol!(sym_key_secretkey_is_set, "key_secretkey_is_set", "29", "2.2.5", r##""##);
symbol!(sym_key_setnet, "key_setnet", "29", "2.2.5", r##""##);
symbol!(sym_key_setsecret, "key_setsecret", "29", "2.2.5", r##""##);
symbol!(sym___libanl_version_placeholder, "__libanl_version_placeholder", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym___libc_sa_len, "__libc_sa_len", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym___libdl_version_placeholder, "__libdl_version_placeholder", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym___libpthread_version_placeholder, "__libpthread_version_placeholder", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym___librt_version_placeholder, "__librt_version_placeholder", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym___libutil_version_placeholder, "__libutil_version_placeholder", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_loc1, "loc1", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_loc2, "loc2", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_locs, "locs", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_netname2host, "netname2host", "29", "2.2.5", r##""##);
symbol!(sym_netname2user, "netname2user", "29", "2.2.5", r##""##);
symbol!(sym_nis_add, "nis_add", "29", "2.2.5", r##""##);
symbol!(sym_nis_add_entry, "nis_add_entry", "29", "2.2.5", r##""##);
symbol!(sym_nis_addmember, "nis_addmember", "29", "2.2.5", r##""##);
symbol!(sym_nis_checkpoint, "nis_checkpoint", "29", "2.2.5", r##""##);
symbol!(sym_nis_clone_directory, "nis_clone_directory", "29", "2.2.5", r##""##);
symbol!(sym_nis_clone_object, "nis_clone_object", "29", "2.2.5", r##""##);
symbol!(sym_nis_clone_result, "nis_clone_result", "29", "2.2.5", r##""##);
symbol!(sym_nis_creategroup, "nis_creategroup", "29", "2.2.5", r##""##);
symbol!(sym___nis_default_access, "__nis_default_access", "29", "2.2.5", r##""##);
symbol!(sym___nis_default_group, "__nis_default_group", "29", "2.2.5", r##""##);
symbol!(sym___nis_default_owner, "__nis_default_owner", "29", "2.2.5", r##""##);
symbol!(sym___nis_default_ttl, "__nis_default_ttl", "29", "2.2.5", r##""##);
symbol!(sym_nis_destroy_object, "nis_destroy_object", "29", "2.2.5", r##""##);
symbol!(sym_nis_destroygroup, "nis_destroygroup", "29", "2.2.5", r##""##);
symbol!(sym_nis_dir_cmp, "nis_dir_cmp", "29", "2.2.5", r##""##);
symbol!(sym_nis_domain_of, "nis_domain_of", "29", "2.2.5", r##""##);
symbol!(sym_nis_domain_of_r, "nis_domain_of_r", "29", "2.2.5", r##""##);
symbol!(sym___nis_finddirectory, "__nis_finddirectory", "29", "2.2.5", r##""##);
symbol!(sym_nis_first_entry, "nis_first_entry", "29", "2.2.5", r##""##);
symbol!(sym_nis_free_directory, "nis_free_directory", "29", "2.2.5", r##""##);
symbol!(sym_nis_free_object, "nis_free_object", "29", "2.2.5", r##""##);
symbol!(sym_nis_free_request, "nis_free_request", "29", "2.2.5", r##""##);
symbol!(sym_nis_freenames, "nis_freenames", "29", "2.2.5", r##""##);
symbol!(sym_nis_freeresult, "nis_freeresult", "29", "2.2.5", r##""##);
symbol!(sym_nis_freeservlist, "nis_freeservlist", "29", "2.2.5", r##""##);
symbol!(sym_nis_freetags, "nis_freetags", "29", "2.2.5", r##""##);
symbol!(sym_nis_getnames, "nis_getnames", "29", "2.2.5", r##""##);
symbol!(sym_nis_getservlist, "nis_getservlist", "29", "2.2.5", r##""##);
symbol!(sym___nis_hash, "__nis_hash", "29", "2.2.5", r##""##);
symbol!(sym_nis_ismember, "nis_ismember", "29", "2.2.5", r##""##);
symbol!(sym_nis_leaf_of, "nis_leaf_of", "29", "2.2.5", r##""##);
symbol!(sym_nis_leaf_of_r, "nis_leaf_of_r", "29", "2.2.5", r##""##);
symbol!(sym_nis_lerror, "nis_lerror", "29", "2.2.5", r##""##);
symbol!(sym_nis_list, "nis_list", "29", "2.2.5", r##""##);
symbol!(sym_nis_local_directory, "nis_local_directory", "29", "2.2.5", r##""##);
symbol!(sym_nis_local_group, "nis_local_group", "29", "2.2.5", r##""##);
symbol!(sym_nis_local_host, "nis_local_host", "29", "2.2.5", r##""##);
symbol!(sym_nis_local_principal, "nis_local_principal", "29", "2.2.5", r##""##);
symbol!(sym_nis_lookup, "nis_lookup", "29", "2.2.5", r##""##);
symbol!(sym_nis_mkdir, "nis_mkdir", "29", "2.2.5", r##""##);
symbol!(sym_nis_modify, "nis_modify", "29", "2.2.5", r##""##);
symbol!(sym_nis_modify_entry, "nis_modify_entry", "29", "2.2.5", r##""##);
symbol!(sym_nis_name_of, "nis_name_of", "29", "2.2.5", r##""##);
symbol!(sym_nis_name_of_r, "nis_name_of_r", "29", "2.2.5", r##""##);
symbol!(sym_nis_next_entry, "nis_next_entry", "29", "2.2.5", r##""##);
symbol!(sym_nis_perror, "nis_perror", "29", "2.2.5", r##""##);
symbol!(sym_nis_ping, "nis_ping", "29", "2.2.5", r##""##);
symbol!(sym_nis_print_directory, "nis_print_directory", "29", "2.2.5", r##""##);
symbol!(sym_nis_print_entry, "nis_print_entry", "29", "2.2.5", r##""##);
symbol!(sym_nis_print_group, "nis_print_group", "29", "2.2.5", r##""##);
symbol!(sym_nis_print_group_entry, "nis_print_group_entry", "29", "2.2.5", r##""##);
symbol!(sym_nis_print_link, "nis_print_link", "29", "2.2.5", r##""##);
symbol!(sym_nis_print_object, "nis_print_object", "29", "2.2.5", r##""##);
symbol!(sym_nis_print_result, "nis_print_result", "29", "2.2.5", r##""##);
symbol!(sym_nis_print_rights, "nis_print_rights", "29", "2.2.5", r##""##);
symbol!(sym_nis_print_table, "nis_print_table", "29", "2.2.5", r##""##);
symbol!(sym_nis_read_obj, "nis_read_obj", "29", "2.2.5", r##""##);
symbol!(sym_nis_remove, "nis_remove", "29", "2.2.5", r##""##);
symbol!(sym_nis_remove_entry, "nis_remove_entry", "29", "2.2.5", r##""##);
symbol!(sym_nis_removemember, "nis_removemember", "29", "2.2.5", r##""##);
symbol!(sym_nis_rmdir, "nis_rmdir", "29", "2.2.5", r##""##);
symbol!(sym_nis_servstate, "nis_servstate", "29", "2.2.5", r##""##);
symbol!(sym_nis_sperrno, "nis_sperrno", "29", "2.2.5", r##""##);
symbol!(sym_nis_sperror, "nis_sperror", "29", "2.2.5", r##""##);
symbol!(sym_nis_sperror_r, "nis_sperror_r", "29", "2.2.5", r##""##);
symbol!(sym_nis_stats, "nis_stats", "29", "2.2.5", r##""##);
symbol!(sym_nis_verifygroup, "nis_verifygroup", "29", "2.2.5", r##""##);
symbol!(sym_nis_write_obj, "nis_write_obj", "29", "2.2.5", r##""##);
symbol!(sym___nisbind_connect, "__nisbind_connect", "29", "2.2.5", r##""##);
symbol!(sym___nisbind_create, "__nisbind_create", "29", "2.2.5", r##""##);
symbol!(sym___nisbind_destroy, "__nisbind_destroy", "29", "2.2.5", r##""##);
symbol!(sym___nisbind_next, "__nisbind_next", "29", "2.2.5", r##""##);
symbol!(sym__null_auth, "_null_auth", "29", "2.2.5", r##""##);
symbol!(sym_passwd2des, "passwd2des", "29", "2.2.5", r##""##);
symbol!(sym_pmap_getmaps, "pmap_getmaps", "29", "2.2.5", r##""##);
symbol!(sym_pmap_getport, "pmap_getport", "29", "2.2.5", r##""##);
symbol!(sym_pmap_rmtcall, "pmap_rmtcall", "29", "2.2.5", r##""##);
symbol!(sym_pmap_set, "pmap_set", "29", "2.2.5", r##""##);
symbol!(sym_pmap_unset, "pmap_unset", "29", "2.2.5", r##""##);
symbol!(sym_putmsg, "putmsg", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_putpmsg, "putpmsg", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_readColdStartFile, "readColdStartFile", "29", "2.2.5", r##""##);
symbol!(sym_registerrpc, "registerrpc", "29", "2.2.5", r##""##);
symbol!(sym_rpc_createerr, "rpc_createerr", "29", "2.2.5", r##""##);
symbol!(sym__rpc_dtablesize, "_rpc_dtablesize", "29", "2.2.5", r##""##);
symbol!(sym___rpc_thread_createerr, "__rpc_thread_createerr", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym___rpc_thread_svc_fdset, "__rpc_thread_svc_fdset", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym___rpc_thread_svc_max_pollfd, "__rpc_thread_svc_max_pollfd", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym___rpc_thread_svc_pollfd, "__rpc_thread_svc_pollfd", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym_rtime, "rtime", "29", "2.2.5", r##"(no installed header declares it)"##);
symbol!(sym___rtld_version_placeholder, "__rtld_version_placeholder", "29", "2.34", r##"(no installed header declares it)"##);
symbol!(sym__seterr_reply, "_seterr_reply", "29", "2.2.5", r##""##);
symbol!(sym_step, "step", "29", "2.2.5", r##""##);
symbol!(sym_svc_exit, "svc_exit", "29", "2.2.5", r##""##);
symbol!(sym_svc_fdset, "svc_fdset", "29", "2.2.5", r##""##);
symbol!(sym_svc_getreq, "svc_getreq", "29", "2.2.5", r##""##);
symbol!(sym_svc_getreq_common, "svc_getreq_common", "29", "2.2.5", r##""##);
symbol!(sym_svc_getreq_poll, "svc_getreq_poll", "29", "2.2.5", r##""##);
symbol!(sym_svc_getreqset, "svc_getreqset", "29", "2.2.5", r##""##);
symbol!(sym_svc_max_pollfd, "svc_max_pollfd", "29", "2.2.5", r##""##);
symbol!(sym_svc_pollfd, "svc_pollfd", "29", "2.2.5", r##""##);
symbol!(sym_svc_register, "svc_register", "29", "2.2.5", r##""##);
symbol!(sym_svc_run, "svc_run", "29", "2.2.5", r##""##);
symbol!(sym_svc_sendreply, "svc_sendreply", "29", "2.2.5", r##""##);
symbol!(sym_svc_unregister, "svc_unregister", "29", "2.2.5", r##""##);
symbol!(sym_svcauthdes_stats, "svcauthdes_stats", "29", "2.2.5", r##""##);
symbol!(sym_svcerr_auth, "svcerr_auth", "29", "2.2.5", r##""##);
symbol!(sym_svcerr_decode, "svcerr_decode", "29", "2.2.5", r##""##);
symbol!(sym_svcerr_noproc, "svcerr_noproc", "29", "2.2.5", r##""##);
symbol!(sym_svcerr_noprog, "svcerr_noprog", "29", "2.2.5", r##""##);
symbol!(sym_svcerr_progvers, "svcerr_progvers", "29", "2.2.5", r##""##);
symbol!(sym_svcerr_systemerr, "svcerr_systemerr", "29", "2.2.5", r##""##);
symbol!(sym_svcerr_weakauth, "svcerr_weakauth", "29", "2.2.5", r##""##);
symbol!(sym_svcfd_create, "svcfd_create", "29", "2.2.5", r##""##);
symbol!(sym_svcraw_create, "svcraw_create", "29", "2.2.5", r##""##);
symbol!(sym_svctcp_create, "svctcp_create", "29", "2.2.5", r##""##);
symbol!(sym_svcudp_bufcreate, "svcudp_bufcreate", "29", "2.2.5", r##""##);
symbol!(sym_svcudp_create, "svcudp_create", "29", "2.2.5", r##""##);
symbol!(sym_svcudp_enablecache, "svcudp_enablecache", "29", "2.2.5", r##""##);
symbol!(sym_svcunix_create, "svcunix_create", "29", "2.2.5", r##""##);
symbol!(sym_svcunixfd_create, "svcunixfd_create", "29", "2.2.5", r##""##);
symbol!(sym_user2netname, "user2netname", "29", "2.2.5", r##""##);
symbol!(sym_writeColdStartFile, "writeColdStartFile", "29", "2.2.5", r##""##);
symbol!(sym_xdecrypt, "xdecrypt", "29", "2.2.5", r##""##);
symbol!(sym_xdr_accepted_reply, "xdr_accepted_reply", "29", "2.2.5", r##""##);
symbol!(sym_xdr_array, "xdr_array", "29", "2.2.5", r##""##);
symbol!(sym_xdr_authdes_cred, "xdr_authdes_cred", "29", "2.2.5", r##""##);
symbol!(sym_xdr_authdes_verf, "xdr_authdes_verf", "29", "2.2.5", r##""##);
symbol!(sym_xdr_authunix_parms, "xdr_authunix_parms", "29", "2.2.5", r##""##);
symbol!(sym_xdr_bool, "xdr_bool", "29", "2.2.5", r##""##);
symbol!(sym_xdr_bytes, "xdr_bytes", "29", "2.2.5", r##""##);
symbol!(sym_xdr_callhdr, "xdr_callhdr", "29", "2.2.5", r##""##);
symbol!(sym_xdr_callmsg, "xdr_callmsg", "29", "2.2.5", r##""##);
symbol!(sym_xdr_cback_data, "xdr_cback_data", "29", "2.2.5", r##""##);
symbol!(sym_xdr_char, "xdr_char", "29", "2.2.5", r##""##);
symbol!(sym_xdr_cryptkeyarg, "xdr_cryptkeyarg", "29", "2.2.5", r##""##);
symbol!(sym_xdr_cryptkeyarg2, "xdr_cryptkeyarg2", "29", "2.2.5", r##""##);
symbol!(sym_xdr_cryptkeyres, "xdr_cryptkeyres", "29", "2.2.5", r##""##);
symbol!(sym_xdr_des_block, "xdr_des_block", "29", "2.2.5", r##""##);
symbol!(sym_xdr_domainname, "xdr_domainname", "29", "2.2.5", r##""##);
symbol!(sym_xdr_double, "xdr_double", "29", "2.2.5", r##""##);
symbol!(sym_xdr_enum, "xdr_enum", "29", "2.2.5", r##""##);
symbol!(sym_xdr_float, "xdr_float", "29", "2.2.5", r##""##);
symbol!(sym_xdr_free, "xdr_free", "29", "2.2.5", r##""##);
symbol!(sym_xdr_getcredres, "xdr_getcredres", "29", "2.2.5", r##""##);
symbol!(sym_xdr_hyper, "xdr_hyper", "29", "2.2.5", r##""##);
symbol!(sym_xdr_int, "xdr_int", "29", "2.2.5", r##""##);
symbol!(sym_xdr_int16_t, "xdr_int16_t", "29", "2.2.5", r##""##);
symbol!(sym_xdr_int32_t, "xdr_int32_t", "29", "2.2.5", r##""##);
symbol!(sym_xdr_int64_t, "xdr_int64_t", "29", "2.2.5", r##""##);
symbol!(sym_xdr_int8_t, "xdr_int8_t", "29", "2.2.5", r##""##);
symbol!(sym_xdr_key_netstarg, "xdr_key_netstarg", "29", "2.2.5", r##""##);
symbol!(sym_xdr_key_netstres, "xdr_key_netstres", "29", "2.2.5", r##""##);
symbol!(sym_xdr_keybuf, "xdr_keybuf", "29", "2.2.5", r##""##);
symbol!(sym_xdr_keydat, "xdr_keydat", "29", "2.2.5", r##""##);
symbol!(sym_xdr_keystatus, "xdr_keystatus", "29", "2.2.5", r##""##);
symbol!(sym_xdr_long, "xdr_long", "29", "2.2.5", r##""##);
symbol!(sym_xdr_longlong_t, "xdr_longlong_t", "29", "2.2.5", r##""##);
symbol!(sym_xdr_mapname, "xdr_mapname", "29", "2.2.5", r##""##);
symbol!(sym_xdr_netnamestr, "xdr_netnamestr", "29", "2.2.5", r##""##);
symbol!(sym_xdr_netobj, "xdr_netobj", "29", "2.2.5", r##""##);
symbol!(sym_xdr_obj_p, "xdr_obj_p", "29", "2.2.5", r##""##);
symbol!(sym_xdr_opaque, "xdr_opaque", "29", "2.2.5", r##""##);
symbol!(sym_xdr_opaque_auth, "xdr_opaque_auth", "29", "2.2.5", r##""##);
symbol!(sym_xdr_peername, "xdr_peername", "29", "2.2.5", r##""##);
symbol!(sym_xdr_pmap, "xdr_pmap", "29", "2.2.5", r##""##);
symbol!(sym_xdr_pmaplist, "xdr_pmaplist", "29", "2.2.5", r##""##);
symbol!(sym_xdr_pointer, "xdr_pointer", "29", "2.2.5", r##""##);
symbol!(sym_xdr_quad_t, "xdr_quad_t", "29", "2.3.4", r##""##);
symbol!(sym_xdr_reference, "xdr_reference", "29", "2.2.5", r##""##);
symbol!(sym_xdr_rejected_reply, "xdr_rejected_reply", "29", "2.2.5", r##""##);
symbol!(sym_xdr_replymsg, "xdr_replymsg", "29", "2.2.5", r##""##);
symbol!(sym_xdr_rmtcall_args, "xdr_rmtcall_args", "29", "2.2.5", r##""##);
symbol!(sym_xdr_rmtcallres, "xdr_rmtcallres", "29", "2.2.5", r##""##);
symbol!(sym_xdr_short, "xdr_short", "29", "2.2.5", r##""##);
symbol!(sym_xdr_sizeof, "xdr_sizeof", "29", "2.2.5", r##""##);
symbol!(sym_xdr_string, "xdr_string", "29", "2.2.5", r##""##);
symbol!(sym_xdr_u_char, "xdr_u_char", "29", "2.2.5", r##""##);
symbol!(sym_xdr_u_hyper, "xdr_u_hyper", "29", "2.2.5", r##""##);
symbol!(sym_xdr_u_int, "xdr_u_int", "29", "2.2.5", r##""##);
symbol!(sym_xdr_u_long, "xdr_u_long", "29", "2.2.5", r##""##);
symbol!(sym_xdr_u_longlong_t, "xdr_u_longlong_t", "29", "2.2.5", r##""##);
symbol!(sym_xdr_u_quad_t, "xdr_u_quad_t", "29", "2.3.4", r##""##);
symbol!(sym_xdr_u_short, "xdr_u_short", "29", "2.2.5", r##""##);
symbol!(sym_xdr_uint16_t, "xdr_uint16_t", "29", "2.2.5", r##""##);
symbol!(sym_xdr_uint32_t, "xdr_uint32_t", "29", "2.2.5", r##""##);
symbol!(sym_xdr_uint64_t, "xdr_uint64_t", "29", "2.2.5", r##""##);
symbol!(sym_xdr_uint8_t, "xdr_uint8_t", "29", "2.2.5", r##""##);
symbol!(sym_xdr_union, "xdr_union", "29", "2.2.5", r##""##);
symbol!(sym_xdr_unixcred, "xdr_unixcred", "29", "2.2.5", r##""##);
symbol!(sym_xdr_valdat, "xdr_valdat", "29", "2.2.5", r##""##);
symbol!(sym_xdr_vector, "xdr_vector", "29", "2.2.5", r##""##);
symbol!(sym_xdr_void, "xdr_void", "29", "2.2.5", r##""##);
symbol!(sym_xdr_wrapstring, "xdr_wrapstring", "29", "2.2.5", r##""##);
symbol!(sym_xdr_yp_buf, "xdr_yp_buf", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypall, "xdr_ypall", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypbind_binding, "xdr_ypbind_binding", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypbind_resp, "xdr_ypbind_resp", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypbind_resptype, "xdr_ypbind_resptype", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypbind_setdom, "xdr_ypbind_setdom", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypdelete_args, "xdr_ypdelete_args", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypmap_parms, "xdr_ypmap_parms", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypmaplist, "xdr_ypmaplist", "29", "2.2.5", r##""##);
symbol!(sym_xdr_yppush_status, "xdr_yppush_status", "29", "2.2.5", r##""##);
symbol!(sym_xdr_yppushresp_xfr, "xdr_yppushresp_xfr", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypreq_key, "xdr_ypreq_key", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypreq_nokey, "xdr_ypreq_nokey", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypreq_xfr, "xdr_ypreq_xfr", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypresp_all, "xdr_ypresp_all", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypresp_key_val, "xdr_ypresp_key_val", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypresp_maplist, "xdr_ypresp_maplist", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypresp_master, "xdr_ypresp_master", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypresp_order, "xdr_ypresp_order", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypresp_val, "xdr_ypresp_val", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypresp_xfr, "xdr_ypresp_xfr", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypstat, "xdr_ypstat", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypupdate_args, "xdr_ypupdate_args", "29", "2.2.5", r##""##);
symbol!(sym_xdr_ypxfrstat, "xdr_ypxfrstat", "29", "2.2.5", r##""##);
symbol!(sym_xdrmem_create, "xdrmem_create", "29", "2.2.5", r##""##);
// §29.1 record and stdio streams (LG-13), against spec/behaviour/xdr_streams.glibc.txt.
// The callbacks are C functions over the test thread's own pipe (a thread-local, so no pointer
// is read here); bytes cross through the memcpy export.
#[allow(clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, unreachable_pub)]
mod xdr_streams {
    use core::cell::RefCell;
    use core::ffi::{c_char, c_int, c_uint};
    use liella_boundary::posix::s04_strings::memcpy;
    use liella_boundary::posix::s12_stdio as io;
    use liella_boundary::posix::s29_legacy::{Xdr, XdrOps};
    use liella_boundary::posix::s29_xdr_streams as xs;

    struct State {
        data: Vec<u8>,
        off: usize,
        chunk: usize,
        writes: Vec<Vec<u8>>,
        fail_writes: bool,
    }

    thread_local! {
        static PIPE: RefCell<State> = const { RefCell::new(State { data: Vec::new(), off: 0, chunk: 0, writes: Vec::new(), fail_writes: false }) };
    }

    /// The test thread's pipe (one at a time per test thread).
    pub struct Pipe;

    impl Pipe {
        pub fn writes(&self) -> Vec<Vec<u8>> {
            PIPE.with_borrow(|s| s.writes.clone())
        }
        pub fn fail_writes(&mut self) {
            PIPE.with_borrow_mut(|s| s.fail_writes = true);
        }
    }

    extern "C" fn rd(_h: *mut c_char, b: *mut c_char, n: c_int) -> c_int {
        PIPE.with_borrow_mut(|p| {
            let k = (p.data.len() - p.off).min(n as usize).min(p.chunk);
            if k == 0 {
                return -1;
            }
            memcpy(b.cast(), p.data.get(p.off..).unwrap_or_default().as_ptr().cast(), k);
            p.off += k;
            k as c_int
        })
    }

    extern "C" fn wr(_h: *mut c_char, b: *mut c_char, n: c_int) -> c_int {
        let mut v = vec![0u8; n as usize];
        memcpy(v.as_mut_ptr().cast(), b.cast_const().cast(), v.len());
        PIPE.with_borrow_mut(|p| {
            p.writes.push(v);
            if p.fail_writes { -1 } else { n }
        })
    }

    pub fn pipe(data: &str, chunk: usize) -> Pipe {
        PIPE.with_borrow_mut(|s| *s = State { data: hex(data), off: 0, chunk, writes: Vec::new(), fail_writes: false });
        Pipe
    }

    pub fn hex(s: &str) -> Vec<u8> {
        let s: Vec<u8> = s.bytes().filter(|b| *b != b' ').collect();
        s.chunks(2).map(|p| u8::from_str_radix(core::str::from_utf8(p).unwrap(), 16).unwrap()).collect()
    }

    pub fn unhex(b: &[u8]) -> String {
        b.iter().map(|c| format!("{c:02x}")).collect()
    }

    pub fn blank(op: c_int) -> Xdr {
        Xdr { x_op: op, x_ops: core::ptr::null(), x_public: core::ptr::null_mut(), x_private: core::ptr::null_mut(), x_base: core::ptr::null_mut(), x_handy: 0 }
    }

    pub fn rec(op: c_int, send: c_uint, _p: &mut Pipe) -> Xdr {
        let mut x = blank(op);
        xs::liella_compat_xdrrec_create(&mut x, send, 0, core::ptr::null_mut(), Some(rd), Some(wr));
        assert!(!x.x_ops.is_null(), "created");
        x
    }

    /// The stream's table, named by identity (x_ops is never read through here).
    pub fn ops(x: &Xdr) -> &'static XdrOps {
        [&xs::REC_OPS, &xs::STDIO_OPS].into_iter().find(|t| core::ptr::eq(*t, x.x_ops)).unwrap()
    }

    pub fn put(x: &mut Xdr, v: i32) -> c_int {
        (ops(x).x_putint32.unwrap())(x, &v)
    }

    /// "ok/value" as the probe prints it (the value stays 0x5a on failure).
    pub fn get(x: &mut Xdr) -> String {
        let mut v = 0x5a;
        let ok = (ops(x).x_getint32.unwrap())(x, &mut v);
        format!("{ok}/{v:x}")
    }

    pub fn destroy(x: &mut Xdr) {
        (ops(x).x_destroy.unwrap())(x)
    }

    pub fn tmp(name: &str, bytes: &[u8]) -> std::ffi::CString {
        let path = std::env::temp_dir().join(format!("liella-xdr-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        std::ffi::CString::new(path.to_str().unwrap()).unwrap()
    }

    pub use io::{fclose, fopen};
}

#[test]
fn sym_xdrrec_create() {
    use xdr_streams::*;
    // rec enc send=0 n=2 now=1: one write of the whole record.
    let mut p = pipe("", 64);
    let mut x = rec(0, 0, &mut p);
    assert_eq!((put(&mut x, 1), put(&mut x, 2)), (1, 1));
    assert_eq!(liella_boundary::posix::s29_xdr_streams::liella_compat_xdrrec_endofrecord(&mut x, 1), 1);
    assert_eq!(p.writes().iter().map(|w| unhex(w)).collect::<Vec<_>>(), ["800000080000000100000002"]);
    // rec enc inline8=ptr inline100000=NULL setpostn0=0
    let inline = ops(&x).x_inline.unwrap();
    assert!(!inline(&mut x, 8).is_null() && inline(&mut x, 100_000).is_null());
    assert_eq!((ops(&x).x_setpostn.unwrap())(&mut x, 0), 0);
    destroy(&mut x);
    // rec dec no-skip first=0/5a: a read before skiprecord fails.
    let mut p = pipe("80000004 00000001", 64);
    let mut x = rec(1, 0, &mut p);
    assert_eq!(get(&mut x), "0/5a");
    destroy(&mut x);
}

#[test]
fn sym_xdrrec_endofrecord() {
    use liella_boundary::posix::s29_xdr_streams::liella_compat_xdrrec_endofrecord as eor;
    use xdr_streams::*;
    // send=0 n=2 now=0: nothing written yet.
    let mut p = pipe("", 64);
    let mut x = rec(0, 0, &mut p);
    assert_eq!((put(&mut x, 1), put(&mut x, 2), eor(&mut x, 0)), (1, 1, 1));
    assert!(p.writes().is_empty());
    destroy(&mut x);
    // send=12 n=5 now=1: w24=8000001400000001...05
    let mut p = pipe("", 64);
    let mut x = rec(0, 12, &mut p);
    for v in 1..=5 {
        assert_eq!(put(&mut x, v), 1);
    }
    assert_eq!(eor(&mut x, 1), 1);
    assert_eq!(unhex(&p.writes().concat()), "800000140000000100000002000000030000000400000005");
    destroy(&mut x);
    // writefail: w8=8000000400000007 ok=1 eor=0
    let mut p = pipe("", 64);
    p.fail_writes();
    let mut x = rec(0, 0, &mut p);
    assert_eq!((put(&mut x, 7), eor(&mut x, 1)), (1, 0));
    assert_eq!(unhex(&p.writes().concat()), "8000000400000007");
    destroy(&mut x);
}

#[test]
fn sym_xdrrec_skiprecord() {
    use liella_boundary::posix::s29_xdr_streams::{liella_compat_xdrrec_eof as eof, liella_compat_xdrrec_skiprecord as skip};
    use xdr_streams::*;
    // rec dec one / one-chunk3 / two-frags: skip0=1 1/1 1/2, then skip=1 eof=1, next=0/5a5a.
    for (data, chunk) in [("80000008 00000001 00000002", 64), ("80000008 00000001 00000002", 3), ("00000004 00000001 80000004 00000002", 64)] {
        let mut p = pipe(data, chunk);
        let mut x = rec(1, 0, &mut p);
        assert_eq!(skip(&mut x), 1, "{data} chunk {chunk}");
        assert_eq!((get(&mut x), get(&mut x)), ("1/1".into(), "1/2".into()), "{data} chunk {chunk}");
        assert_eq!(get(&mut x), "0/5a", "past the record's end");
        assert_eq!((skip(&mut x), eof(&mut x)), (1, 1), "skip=1 eof=1 (the probe's argument order)");
        assert_eq!(get(&mut x), "0/5a", "next: no further record");
        destroy(&mut x);
    }
    // short-data: the record says 8 bytes, 4 come: 1/1, then the read fails.
    let mut p = pipe("80000008 00000001", 64);
    let mut x = rec(1, 0, &mut p);
    assert_eq!((skip(&mut x), get(&mut x), get(&mut x)), (1, "1/1".into(), "0/5a".into()));
    destroy(&mut x);
}

#[test]
fn sym_xdrrec_eof() {
    use liella_boundary::posix::s29_xdr_streams::{liella_compat_xdrrec_eof as eof, liella_compat_xdrrec_skiprecord as skip};
    use xdr_streams::*;
    // skip-unread: eof consumes the rest of the record; nothing buffered after it.
    let mut p = pipe("80000008 00000001 00000002", 64);
    let mut x = rec(1, 0, &mut p);
    assert_eq!((skip(&mut x), eof(&mut x)), (1, 1));
    destroy(&mut x);
    // two records buffered: after the first, eof is 0 (the second is in the buffer).
    let mut p = pipe("80000004 00000001 80000004 00000002", 64);
    let mut x = rec(1, 0, &mut p);
    assert_eq!((skip(&mut x), get(&mut x), eof(&mut x)), (1, "1/1".into(), 0));
    assert_eq!((skip(&mut x), get(&mut x), eof(&mut x)), (1, "1/2".into(), 1));
    destroy(&mut x);
}

#[test]
fn sym_xdrstdio_create() {
    use liella_boundary::posix::s29_xdr_streams::liella_compat_xdrstdio_create as create;
    use xdr_streams::*;
    // stdio enc ok=1 pos=4,24 inline=NULL setpostn0=1 pos=0 file(24)=...
    let path = tmp("enc", b"");
    let fp = fopen(path.as_ptr(), c"w+".as_ptr());
    assert!(!fp.is_null());
    let mut x = blank(0);
    create(&mut x, fp, 0);
    assert_eq!(put(&mut x, 9), 1);
    let getpostn = ops(&x).x_getpostn.unwrap();
    assert_eq!(getpostn(&x), 4);
    let o = ops(&x);
    assert_eq!((o.x_putbytes.unwrap())(&mut x, hex("0102030405060708").as_ptr().cast(), 8), 1);
    assert_eq!(put(&mut x, 5), 1);
    assert_eq!((o.x_putbytes.unwrap())(&mut x, hex("68656c6c6f000000").as_ptr().cast(), 8), 1);
    assert_eq!(getpostn(&x), 24);
    assert!((o.x_inline.unwrap())(&mut x, 4).is_null());
    assert_eq!(((o.x_setpostn.unwrap())(&mut x, 0), getpostn(&x)), (1, 0));
    destroy(&mut x);
    assert_eq!(fclose(fp), 0);
    let bytes = std::fs::read(path.to_str().unwrap()).unwrap();
    assert_eq!(unhex(&bytes), "0000000901020304050607080000000568656c6c6f000000");
    // stdio dec: 1:9, 8 bytes, then past the end 0 (value kept) at pos 24; a zero-length read succeeds.
    let fp = fopen(path.as_ptr(), c"r".as_ptr());
    let mut x = blank(1);
    create(&mut x, fp, 1);
    assert_eq!(get(&mut x), "1/9");
    let mut b = [0u8; 8];
    assert_eq!((ops(&x).x_getbytes.unwrap())(&mut x, b.as_mut_ptr().cast(), 8), 1);
    assert_eq!(unhex(&b), "0102030405060708");
    assert_eq!((get(&mut x), get(&mut x)), ("1/5".into(), "1/68656c6c".into()));
    assert_eq!((ops(&x).x_getbytes.unwrap())(&mut x, b.as_mut_ptr().cast(), 0), 1);
    assert_eq!((get(&mut x), get(&mut x), (ops(&x).x_getpostn.unwrap())(&x)), ("1/6f000000".into(), "0/5a".into(), 24));
    destroy(&mut x);
    assert_eq!(fclose(fp), 0);
    // stdio dec short: 6 bytes: 1:1, then 0 at pos 6.
    let path = tmp("short", &hex("00000001 0000"));
    let fp = fopen(path.as_ptr(), c"r".as_ptr());
    let mut x = blank(1);
    create(&mut x, fp, 1);
    assert_eq!((get(&mut x), get(&mut x), (ops(&x).x_getpostn.unwrap())(&x)), ("1/1".into(), "0/5a".into(), 6));
    destroy(&mut x);
    assert_eq!(fclose(fp), 0);
    let _ = std::fs::remove_file(path.to_str().unwrap());
}
symbol!(sym_xencrypt, "xencrypt", "29", "2.2.5", r##""##);
symbol!(sym_xprt_register, "xprt_register", "29", "2.2.5", r##""##);
symbol!(sym_xprt_unregister, "xprt_unregister", "29", "2.2.5", r##""##);
symbol!(sym_yp_all, "yp_all", "29", "2.2.5", r##""##);
symbol!(sym_yp_bind, "yp_bind", "29", "2.2.5", r##""##);
symbol!(sym___yp_check, "__yp_check", "29", "2.2.5", r##""##);
symbol!(sym_yp_first, "yp_first", "29", "2.2.5", r##""##);
symbol!(sym_yp_get_default_domain, "yp_get_default_domain", "29", "2.2.5", r##""##);
symbol!(sym_yp_maplist, "yp_maplist", "29", "2.2.5", r##""##);
symbol!(sym_yp_master, "yp_master", "29", "2.2.5", r##""##);
symbol!(sym_yp_match, "yp_match", "29", "2.2.5", r##""##);
symbol!(sym_yp_next, "yp_next", "29", "2.2.5", r##""##);
symbol!(sym_yp_order, "yp_order", "29", "2.2.5", r##""##);
symbol!(sym_yp_unbind, "yp_unbind", "29", "2.2.5", r##""##);
symbol!(sym_yp_update, "yp_update", "29", "2.2.5", r##""##);
symbol!(sym_ypbinderr_string, "ypbinderr_string", "29", "2.2.5", r##""##);
symbol!(sym_yperr_string, "yperr_string", "29", "2.2.5", r##""##);
symbol!(sym_ypprot_err, "ypprot_err", "29", "2.2.5", r##""##);
