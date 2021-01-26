use std::{env, ffi::OsString, fmt::Display, io::Error, io::ErrorKind, path::PathBuf, process::Command, time::Duration};

use rand::thread_rng;
use rand::seq::SliceRandom;

#[derive(Debug)]
enum DummyError {
	InvalidArgument
}
impl Display for DummyError {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}
impl std::error::Error for DummyError {

}

trait NestedInto<T> {
	fn n_into(&self) -> T;
}

impl<'a, I, F> NestedInto<Option<I>> for Option<F>
	where F: Into<I>, F: Clone {
	fn n_into(&self) -> Option<I> {
		if let Some(val) = self {
			Some(val.clone().into())
		} else {
			None
		}
	}
}

const TARGETARG: &'static str = "--target";
const SOURCEARG: &'static str = "--source";
const TEMPARG: &'static str = "--temp";
const PROGRAM: &'static str = "--program";

fn main() -> Result<(), Error> {
	// reading arguments
	let mut dummy_mode: bool = false;
	let mut auto_recover_mode: bool = false;
	let mut target_dir: Option<String> = None;
	let mut source_dir: Option<String> = None;
	// if target is specified but source is not, we assume they are the same directory.
	let mut auto_assigned_source = false;
	let mut temp_dir: Option<String> = None;
	let mut program: Option<String> = None;
	//TODO: Actually implement this
	let mut exhaustive = false;
	let mut arg_iter = env::args();
	arg_iter.next(); //the first argument is the program name which we don't give a shit about.
	loop {
		let arg = match arg_iter.next() {
			Some(val) => val,
			None => break,
		};
		// On windows it doesn't matter if we lowercase path names, but linux is case sensitive
		// so we're only using the lowercased version for matching arguments
		match &*arg.to_lowercase() {
			TARGETARG => {
				if let Some(_) = target_dir {
					eprintln!("Warning: Reassigning target directory due to explicit assignment; check your arguments");
				}
				target_dir = Some(arg_iter.next().expect(&*format!("Expected argument for '{}'", TARGETARG)));
				if let None = source_dir {
					source_dir = target_dir.clone();
					auto_assigned_source = true;
				}
			},
			SOURCEARG => {
				if !auto_assigned_source {
					if let Some(_) = source_dir {
						eprintln!("Warning: Reassigning source directory due to explicit assignment; check your arguments");
					}
				}
				source_dir = Some(arg_iter.next().expect(&*format!("Expected argument for '{}'", SOURCEARG)));
				auto_assigned_source = false;
			},
			TEMPARG => {
				if let Some(_) = temp_dir {
					eprintln!("Warning: Reassigning temporary directory due to explicit assignment; check your arguments");
				}
				temp_dir = Some(arg_iter.next().expect(&*format!("Expected argument for '{}'", TEMPARG)));
			},
			PROGRAM => {
				if let Some(_) = program {
					eprintln!("Warning: Reassigning target program due to explicit assignment; check your arguments");
				}
				program = Some(arg_iter.next().expect(&*format!("Expected argument for '{}'", PROGRAM)));
			},
			"--exhaustive" | "-e" => {
				exhaustive = true;
			},
			"--dummy" | "-d" => {
				dummy_mode = true;
			},
			"--autorecover" | "-a" => {
				auto_recover_mode = true;
			},
			"--help" | "-h" => {
				print_help();
				return Ok(());
			},
			_ => {
				// fill in any variables not set by name in order, starting from target_dir.
				// we auto-set source_dir to be target_dir if not explicitly set, but also
				// mark it as auto-set so we can later set it implicitly.
				if target_dir.is_none() {
					target_dir = Some(arg);
					if source_dir.is_none() {
						source_dir = target_dir.clone();
						auto_assigned_source = true;
					}
				} else if source_dir.is_none() || auto_assigned_source {
					source_dir = Some(arg);
					auto_assigned_source = false;
				} else if temp_dir.is_none() {
					temp_dir = Some(arg);
				} else if program.is_none() {
					program = Some(arg);
				} else {
					eprintln!("Warning: extra (unused) parameter detected: {}", arg);
				}
			}
		}
	}
	// Unwrapping directory/program targets with default values if unspecified
	let target_dir: PathBuf = target_dir.unwrap_or("D:\\Documents\\Sims Mods".into()).into();
	let source_dir: PathBuf = source_dir.n_into().unwrap_or_else(|| target_dir.clone());
	let temp_dir: PathBuf = if let Some(dir) = temp_dir {
		dir.into()
	} else {
		if let Some(t_dir) = target_dir.parent() {
			let mut r = t_dir.clone().to_path_buf();
			r.push("temp");
			r
		} else {
			eprintln!("Error: Missing argument \"temp_dir\" and could not assume a default one");
			print_help();
			return create_error();
		}
	};
	let program: PathBuf = program.unwrap_or("C:\\Program Files (x86)\\Origin Games\\The Sims 4\\Game\\Bin\\TS4_x64.exe".into()).into();

	// test each path and ensure all are valid, or throw a usage exception
	if !target_dir.exists() {
		eprintln!("Error: Target Directory \"{:?}\" does not exist", target_dir);
		print_help();
		return create_error();
	}
	if !source_dir.exists() {
		eprintln!("Error: Mod Source Directory \"{:?}\" does not exist", source_dir);
		print_help();
		return create_error();
	}
	if !temp_dir.exists() {
		eprintln!("Error: Temporary Directory \"{:?}\" does not exist", temp_dir);
		print_help();
		return create_error();
	}
	if !program.exists() {
		eprintln!("Error: Program \"{:?}\" does not exist", program);
		print_help();
		return create_error();
	}

	let mut program = Command::new(program);
	// move the mods to the temp dir and put handles to all of them in a vector
	let mut mods = prepare_mods(source_dir.clone(), temp_dir.clone())?;
	if mods.len() == 0 {
		if !auto_recover_mode {
			panic!("Mods source directory was empty!");
		} else {
			mods = prepare_mods(temp_dir.clone(), temp_dir.clone())?;
		}
	}
	move_all_to_target(&mut mods, temp_dir.clone(), target_dir.clone(), false)?;
	// shuffling the list to help allow for finding mod incompatibility problems
	// it's still not GOOD at it but this at least makes it feasible
	mods.shuffle(&mut thread_rng());
	
	let mut continue_searching = true;
	while continue_searching {
		if exhaustive {
			let result = exhaustive_search_for_broken_mod(
				&mut program,
				temp_dir.clone(),
				target_dir.clone(),
				&mut mods[..],
				dummy_mode,
			)?;
			for result in result {
				if let ProblemCauser::SingleMod(broken_mod) = result {
					println!("Found a broken mod: {:?}", broken_mod);
				} else if let ProblemCauser::ModCombo(broken_mods) = result {
					println!("Found multiple broken mods (only broken when together): {:?}", broken_mods);
				}
			}
			println!("Continue searching?");
			if !get_user_bool_input() {
				continue_searching = false;
			} else {
				move_all_to_target(&mut mods, temp_dir.clone(), target_dir.clone(), false)?;
			}
		} else {
			let result = search_for_broken_mod(
				&mut program,
				temp_dir.clone(),
				target_dir.clone(),
				&mut mods[..],
				dummy_mode,
			)?;
			if let ProblemCauser::SingleMod(broken_mod) = result {
				println!("Found a broken mod: {:?}", broken_mod);
			} else if let ProblemCauser::ModCombo(broken_mods) = result {
				println!("Found multiple broken mods: {:?}", broken_mods);
			} 
			println!("Continue searching?");
			if !get_user_bool_input() {
				continue_searching = false;
			} else {
				move_all_to_target(&mut mods, temp_dir.clone(), target_dir.clone(), false)?;
			}
		}
	}
	move_all_out_of_target(&mut mods, temp_dir.clone(), target_dir.clone(), true)?;
	move_all_to_target(&mut mods, temp_dir.clone(), source_dir.clone(), true)?;
	Ok(())
}

fn create_error() -> Result<(), Error> {
	Err(Error::new(ErrorKind::InvalidInput, Box::new(DummyError::InvalidArgument)))
}

fn prepare_mods(source_dir: PathBuf, temp_dir: PathBuf) -> Result<Vec<OsString>, Error> {
	let mut mods = Vec::new();
	let mods_start = std::fs::read_dir(source_dir.clone())?;
	let _ = std::fs::create_dir_all(temp_dir.clone());
	for mod_file in mods_start.into_iter() {
		let mod_file = mod_file?;
		let mut target_file_path = temp_dir.clone();
		target_file_path.push(mod_file.file_name());
		std::fs::rename(mod_file.path(), target_file_path)?;
		mods.push(mod_file.file_name());
	}
	Ok(mods)
}

fn print_help() {
	println!("Tests mods using an unsorted binary search, using you to test each branch.");
	println!("Usage:");
	println!("modtester [target] [source] [temp_dir] [program]");
	println!("Users can also specify each argument by name like so:");
	println!("--target <target>");
	println!("--source <source>");
	println!("--temp_dir <temp_dir>");
	println!("--program <program>");
	println!("Arguments specified by name do not need to be in order.");
	println!("If an argument has already been specified by name, the matching on unnamed arguments will skip it.");
	println!("For example: Running the command \"modtester --source C:/Uglystinky C:/Facelikearat C:/donkeylookingman\"");
	println!("will result in the target being set to \"C:/Facelikearat\" and the temp dir being set to \"C:/donkeylookingman\".");
	println!("");
	println!("Additional optional arguments:");
	println!("--help       | -h   | Prints this help dialog.");
	println!("--exhaustive | -e   | Enables exhaustive searching, which will find ALL broken mods instead of just the first.");
	println!("--dummy      | -d   | Enables \"dummy mode\", which will not run the program (but will move the files); mostly a debugging tool.");
}


/// # Parameters
/// `safety_net` should be true if you are not sure all files exist in the temp folder and are just ensuring all files are in the target.
fn move_all_to_target(
	mods_list: &mut [OsString],
	temp_dir: PathBuf,
	target_dir: PathBuf,
	safety_net: bool,
) -> Result<(), Error> {
	for mod_file in mods_list.iter() {
		let mut temp_dir_file_path = temp_dir.clone();
		temp_dir_file_path.push(mod_file);
		let mut target_dir_file_path = target_dir.clone();
		target_dir_file_path.push(mod_file);
		if temp_dir_file_path.exists() {
			match std::fs::rename(temp_dir_file_path.clone(), target_dir_file_path.clone()) {
			   Ok(_) => {}
			   Err(err) => {
					if err.kind() == ErrorKind::PermissionDenied || err.raw_os_error().unwrap_or(0) == 32 {
						std::thread::sleep(Duration::from_secs(5));
						std::fs::rename(temp_dir_file_path, target_dir_file_path)?
					} else {
						return Err(err);
					}
				}
			}
		} else if !safety_net {
			eprintln!("Warning: File did not exist. Dev is probably stupid.");
		}
	}
	Ok(())
}
fn move_all_out_of_target(
	mods_list: &mut [OsString],
	temp_dir: PathBuf,
	target_dir: PathBuf,
	safety_net: bool,
) -> Result<(), Error> {
	move_all_to_target(mods_list, target_dir, temp_dir, safety_net)
}

enum ProblemCauser<T> {
	SingleMod(T),
	ModCombo(Vec<T>),
	None
}
impl<T> ProblemCauser<T> {
	fn is_some(&self) -> bool {
		match self {
		    ProblemCauser::SingleMod(_) => {true}
		    ProblemCauser::ModCombo(_) => {true}
		    ProblemCauser::None => {false}
		}
	}
}

/// Executes the search for the broken mod recursively.
/// This function is guaranteed to move the entirety of `mods_list` to the temp dir.
/// # Returns
/// Returns whether it found the broken mod down this branch.
fn search_for_broken_mod(
	program: &mut Command,
	temp_dir: PathBuf,
	target_dir: PathBuf,
	mods_list: &mut [OsString],
	dummy_mode: bool,
) -> Result<ProblemCauser<OsString>, Error> {
	if mods_list.len() < 1 {
		panic!("Mods list was empty! This should never be reached!");
	}
	// if the program failed to run, go down another step on the binary search unless the search is already narrowed down to a single mod
	if !test_program(program, dummy_mode) {
		if mods_list.len() == 1 {
			move_all_out_of_target(mods_list, temp_dir, target_dir, false)?;
			return Ok(ProblemCauser::SingleMod(mods_list[0].clone()));
		}
		let midpoint = mods_list.len() / 2;
		move_all_out_of_target(
			&mut mods_list[midpoint..],
			temp_dir.clone(),
			target_dir.clone(),
			false
		)?;
		let ret = search_for_broken_mod(
			program,
			temp_dir.clone(),
			target_dir.clone(),
			&mut mods_list[..midpoint],
			dummy_mode,
		)?;
		if ret.is_some() {
			return Ok(ret);
		} else {
			move_all_to_target(
				&mut mods_list[midpoint..],
				temp_dir.clone(),
				target_dir.clone(),
				false,
			)?;
			let ret = search_for_broken_mod(
				program,
				temp_dir.clone(),
				target_dir.clone(),
				&mut mods_list[midpoint..],
				dummy_mode,
			)?;
			if ret.is_some() {
				return Ok(ret);
			}
		}
		Ok(ProblemCauser::ModCombo(Vec::from(mods_list)))
	} else {
		move_all_out_of_target(mods_list, temp_dir, target_dir, false)?;
		Ok(ProblemCauser::None)
	}
}

/// executes the search for the broken mod recursively.
/// # Returns
/// returns all broken mods (or mod combos) found down this branch.
fn exhaustive_search_for_broken_mod(
	program: &mut Command,
	temp_dir: PathBuf,
	target_dir: PathBuf,
	mods_list: &mut [OsString],
	dummy_mode: bool,
) -> Result<Vec<ProblemCauser<OsString>>, Error> {
	if mods_list.len() < 1 {
		panic!("Mods list was empty! This should never be reached!");
	}
	// if the program failed to run, go down another step on the binary search unless the search is already narrowed down to a single mod
	if !test_program(program, dummy_mode) {
		if mods_list.len() == 1 {
			move_all_out_of_target(mods_list, temp_dir, target_dir, false)?;
			return Ok(vec![ProblemCauser::SingleMod(mods_list[0].clone())]);
		}
		let midpoint = mods_list.len() / 2;
		move_all_out_of_target(
			&mut mods_list[midpoint..],
			temp_dir.clone(),
			target_dir.clone(),
			false,
		)?;
		let mut ret_a = exhaustive_search_for_broken_mod(
			program,
			temp_dir.clone(),
			target_dir.clone(),
			&mut mods_list[..midpoint],
			dummy_mode,
		)?;
		move_all_to_target(
			&mut mods_list[midpoint..],
			temp_dir.clone(),
			target_dir.clone(),
			false,
		)?;
		let mut ret_b = exhaustive_search_for_broken_mod(
			program,
			temp_dir.clone(),
			target_dir.clone(),
			&mut mods_list[midpoint..],
			dummy_mode,
		)?;
		if ret_a.is_empty() && ret_b.is_empty() {
			return Ok(vec![ProblemCauser::ModCombo(Vec::from(mods_list))]);
		}
		let mut ret = Vec::new();
		ret.append(&mut ret_a);
		ret.append(&mut ret_b);
		return Ok(ret);
	}
	Ok(vec![])
}

/// Tests the program.
/// # Returns
/// Returns whether the program executed successfully.
fn test_program(program: &mut Command, dummy_mode: bool) -> bool {
	println!("Running program...");
	if !dummy_mode {
		program.output().expect("Failed to run program");
	}
	println!("Did the program run successfully?");
	get_user_bool_input()
}

fn get_user_bool_input() -> bool {
	let mut input = String::new();

	std::io::stdin()
		.read_line(&mut input)
		.expect("Failed to read line");
	input.to_lowercase().contains('y')
}