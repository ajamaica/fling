use fling_cli::{
    commands,
    config::Config,
    error::{Error, json_failure},
    install, json_api, process, runtime, watcher,
};
use std::{env, process::ExitCode};
fn need(args: &[String], n: usize) -> bool {
    args.len() == n
}
fn run() -> Result<(), Error> {
    let args: Vec<String> = env::args().collect();
    let c = Config::load()?;
    let a = args.get(1).map(String::as_str).unwrap_or("");
    match a{
"games" if args.get(2).map(String::as_str)==Some("--json")&&need(&args,3)=>json_api::games(&c,false),
"status" if args.get(2).map(String::as_str)==Some("--json")&&need(&args,3)=>json_api::status(&c,&args[0]),
"games"=>{eprintln!("usage: fling games --json");std::process::exit(2)},
"status"=>{eprintln!("usage: fling status --json");std::process::exit(2)},
"installed" if args.get(2).map(String::as_str)==Some("--json")&&need(&args,3)=>json_api::games(&c,true),
"installed" if need(&args,2)=>commands::installed(&c),"list" if need(&args,2)=>commands::list(&c),
"install" if need(&args,4)&&args[3]=="--json"=>install::install_json(&c,&args[2]),
"remove" if need(&args,4)&&args[3]=="--json"=>install::remove_json(&c,&args[2]),
"refresh" if need(&args,4)&&args[3]=="--json"=>json_api::refresh(&c,&args[2]),
"install"=>json_failure("install",0,2,"invalid_args","usage: fling install <appid> --json"),"remove"=>json_failure("remove",0,2,"invalid_args","usage: fling remove <appid> --json"),"refresh"=>json_failure("refresh",0,2,"invalid_args","usage: fling refresh <appid> --json"),
"get" if args.len()>=3=>install::legacy_get(&c,&args[2..].join(" "))?,"auto" if args.len()>=3=>{let q=args[2..].join(" ");install::legacy_get(&c,&q)?;commands::setup(&c,Some(&q))?},"run" if args.len()>=3=>commands::run(&c,&args[2..].join(" "))?,
"setup"|"inject-properties"=>{let query=(args.len()>2).then(||args[2..].join(" "));commands::setup(&c,query.as_deref())?},"restart-steam"=>commands::restart()?,"_steamroot"=>println!("{}",c.steam_root.display()),"_lo-edit" if need(&args,5)=>std::process::exit(commands::lo_edit(&args[2],&args[3],&args[4])?),
"_game-ready" if need(&args,3)=>std::process::exit(process::game_ready(&c,args[2].parse().unwrap_or(0))),"_watch-run" if need(&args,3)=>std::process::exit(watcher::retry(args[2].parse().unwrap_or(0))),"_install-reframework" if need(&args,3)=>runtime::install(&c,args[2].parse().unwrap_or(0))?,"watch"=>watcher::watch(&c)?,
_=>return Err(Error::Message("usage: fling games|status|install|remove|refresh|list|get|auto|run|setup|restart-steam|installed|watch".into()))}
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ERROR: {e}");
            ExitCode::FAILURE
        }
    }
}
