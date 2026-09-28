use fling_cli::{
    commands,
    config::Config,
    error::{Error, json_failure},
    install, json_api, process, runtime, watcher, wemod,
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
"wemod" if args.get(2).map(String::as_str)==Some("status")&&need(&args,3)=>wemod::status(&c),
"wemod" if args.get(2).map(String::as_str)==Some("setup")&&(need(&args,5)||(need(&args,6)&&args[5]=="--dotnet"))=>wemod::setup(&c,&args[3],std::path::Path::new(&args[4]),args.len()==6)?,
"wemod" if args.get(2).map(String::as_str)==Some("dotnet")&&args.len()>=4&&args.last().map(String::as_str)==Some("--json")&&(need(&args,5)||(need(&args,6)&&args[4]=="--check"))=>wemod::dotnet_json(&c,&args[3],args.len()==6),
"wemod" if args.get(2).map(String::as_str)==Some("dotnet")&&(need(&args,4)||(need(&args,5)&&args[4]=="--check"))=>wemod::dotnet(&c,&args[3],args.len()==5)?,
"wemod" if args.get(2).map(String::as_str)==Some("install")&&need(&args,5)&&args[4]=="--json"=>wemod::install_json(&c,&args[3]),
"wemod" if args.get(2).map(String::as_str)==Some("install")&&(need(&args,4)||(need(&args,5)&&args[4]=="--dotnet"))=>wemod::install(&c,&args[3],args.len()==5)?,
"wemod"=>{eprintln!("usage: fling wemod status|install <appid> [--dotnet]|dotnet <appid> [--check]|setup <appid> <WeMod-Setup.exe> [--dotnet]");std::process::exit(2)},
"use" if need(&args,5)&&args[4]=="--json"=>wemod::choose_json(&c,&args[2],&args[3]),
"use" if args.len()>=4&&wemod::Choice::parse(&args[args.len()-1]).is_some()=>wemod::choose(&c,&args[2..args.len()-1].join(" "),Some(&args[args.len()-1]))?,
"use" if args.len()>=3=>wemod::choose(&c,&args[2..].join(" "),None)?,
"use"=>{eprintln!("usage: fling use <game> [fling|wemod|both]");std::process::exit(2)},
_=>return Err(Error::Message("usage: fling games|status|install|remove|refresh|list|get|auto|run|setup|restart-steam|installed|watch|use|wemod".into()))}
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
