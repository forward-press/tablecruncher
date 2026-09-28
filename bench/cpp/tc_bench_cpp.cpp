/*
 * SPDX-License-Identifier: GPL-3.0-or-later
 *
 * Headless benchmark harness for the unmodified Tablecruncher C++ core.
 *
 * Runs the parser, storage, table, save, find, sort and macro code from src/ the same way the
 * app does and prints one JSON object per step. The Rust prototype's `tc-bench` implements the
 * same CLI and output (contract: docs/dev/RUST_PROTOTYPE_HANDOFF.md §7.4), so bench/run-bench
 * can compare both.
 */

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <map>
#include <set>
#include <sstream>
#include <string>
#include <tuple>
#include <vector>

#include "csvparser.hh"
#include "csvtable.hh"
#include "helper.hh"
#include "macro.hh"

#ifdef _WIN32
#include <windows.h>
#include <psapi.h>
#else
#include <sys/resource.h>
#endif


// Globals the core expects; the GUI normally defines them.
Macro macro;
void updateMacroLogBuffer(void *, std::string) {}


namespace {

using Clock = std::chrono::steady_clock;

double peakRssMb() {
#ifdef _WIN32
	PROCESS_MEMORY_COUNTERS pmc;
	GetProcessMemoryInfo(GetCurrentProcess(), &pmc, sizeof(pmc));
	return pmc.PeakWorkingSetSize / (1024.0 * 1024.0);
#else
	struct rusage usage;
	getrusage(RUSAGE_SELF, &usage);
#ifdef __APPLE__
	return usage.ru_maxrss / (1024.0 * 1024.0);		// bytes
#else
	return usage.ru_maxrss / 1024.0;				// KiB
#endif
#endif
}

void noStatus(const char *, void *) {}

// File name (without directories) as reported in every JSON line.
std::string g_file;

std::string baseName(const std::string &path) {
	auto pos = path.find_last_of("/\\");
	return pos == std::string::npos ? path : path.substr(pos + 1);
}

struct Args {
	std::map<std::string, std::string> values;
	std::set<std::string> flags;

	bool has(const std::string &key) const {
		return values.count(key) || flags.count(key);
	}
	std::string get(const std::string &key, const std::string &def = "") const {
		auto it = values.find(key);
		return it == values.end() ? def : it->second;
	}
};

Args parseArgs(int argc, char **argv) {
	static const std::set<std::string> boolFlags = {"save", "sort-desc"};
	Args args;
	for( int i = 1; i < argc; ++i ) {
		std::string key = argv[i];
		if( key.rfind("--", 0) != 0 ) {
			std::fprintf(stderr, "unexpected argument: %s\n", argv[i]);
			std::exit(2);
		}
		key = key.substr(2);
		if( boolFlags.count(key) ) {
			args.flags.insert(key);
		} else if( i + 1 < argc ) {
			args.values[key] = argv[++i];
		} else {
			std::fprintf(stderr, "missing value for --%s\n", key.c_str());
			std::exit(2);
		}
	}
	return args;
}

char charArg(const Args &args, const std::string &key, char def) {
	std::string value = args.get(key);
	if( value.empty() )
		return def;
	return value == "tab" ? '\t' : value[0];
}

CsvDefinition::Encodings encodingArg(const std::string &value) {
	if( value == "latin1" ) return CsvDefinition::ENC_Latin1;
	if( value == "win1252" ) return CsvDefinition::ENC_Win1252;
	if( value == "utf16le" ) return CsvDefinition::ENC_UTF16LE;
	if( value == "utf16be" ) return CsvDefinition::ENC_UTF16BE;
	return CsvDefinition::ENC_UTF8;
}

void report(const char *step, Clock::time_point start, CsvTable &table, const std::string &extra = "") {
	long long ms = std::chrono::duration_cast<std::chrono::milliseconds>(Clock::now() - start).count();
	std::printf("{\"tool\":\"cpp\",\"file\":\"%s\",\"step\":\"%s\",\"ms\":%lld,\"rows\":%d,\"cols\":%d,\"peak_rss_mb\":%.0f%s}\n",
		g_file.c_str(), step, ms, table.getNumberRows(), table.getNumberCols(), peakRssMb(), extra.c_str());
	std::fflush(stdout);
}

bool save(const char *step, CsvTable &table, const std::string &path) {
	auto start = Clock::now();
	if( table.saveCsv(path, noStatus, nullptr) != CsvTable::SAVE_OKAY ) {
		std::fprintf(stderr, "could not save %s\n", path.c_str());
		return false;
	}
	report(step, start, table);
	return true;
}

void find(const char *step, CsvTable &table, const std::string &needle, bool caseSensitive, bool useRegex) {
	std::vector<table_index_t> area = {0, 0, table.getNumberRows() - 1, table.getNumberCols() - 1};
	auto start = Clock::now();
	auto [row, col] = table.findSubstring(needle, 0, 0, area, caseSensitive, useRegex);
	report(step, start, table, ",\"found\":[" + std::to_string(row) + "," + std::to_string(col) + "]");
}

}	// namespace


int main(int argc, char **argv) {
	Args args = parseArgs(argc, argv);
	std::string file = args.get("file");
	std::string out = args.get("out", "out");
	if( file.empty() ) {
		std::fprintf(stderr, "usage: tc_bench_cpp --file F [--delim C|tab] [--quote C] [--escape C] "
			"[--enc utf8|latin1|win1252|utf16le|utf16be] [--bom N] [--out PREFIX] [--save] "
			"[--find T] [--find-ci T] [--find-re P] [--macro F.js --macro-sel r0,c0,r1,c1] "
			"[--sort-col N --sort-type num|str|stri [--sort-desc]]\n");
		return 2;
	}
	g_file = baseName(file);

	CsvDefinition def;
	def.delimiter = charArg(args, "delim", ',');
	def.quote = charArg(args, "quote", '"');
	def.escape = charArg(args, "escape", '"');
	def.encoding = encodingArg(args.get("enc", "utf8"));
	def.bomBytes = std::stoi(args.get("bom", "0"));

	// Same call as CsvWindow::loadFile(): default-constructed ifstream, text mode.
	std::ifstream input;
	input.open(file);
	if( !input ) {
		std::fprintf(stderr, "cannot open %s\n", file.c_str());
		return 1;
	}

	CsvTable table(0, 0);		// not CsvTable(): the default constructor leaves headerRow uninitialised
	auto start = Clock::now();
	CsvParser().parseCsvStream(&input, table.getStorage(), &def);
	table.updateInternals();
	table.setDefinition(def);
	report("load", start, table);

	if( args.has("save") && !save("save", table, out + ".saved.csv") )
		return 1;
	if( args.has("find") )
		find("find_cs", table, args.get("find"), true, false);
	if( args.has("find-ci") )
		find("find_ci", table, args.get("find-ci"), false, false);
	if( args.has("find-re") )
		find("find_re", table, args.get("find-re"), true, true);

	if( args.has("macro") ) {
		std::ifstream js(args.get("macro"), std::ios::binary);
		if( !js ) {
			std::fprintf(stderr, "cannot open macro %s\n", args.get("macro").c_str());
			return 1;
		}
		std::stringstream source;
		source << js.rdbuf();
		int sel[4] = {0, 0, -1, -1};		// negative row/col max = last row/col
		std::sscanf(args.get("macro-sel", "0,0,-1,-1").c_str(), "%d,%d,%d,%d", &sel[0], &sel[1], &sel[2], &sel[3]);
		if( sel[2] < 0 ) sel[2] = table.getNumberRows() - 1;
		if( sel[3] < 0 ) sel[3] = table.getNumberCols() - 1;
		start = Clock::now();
		auto [rc, err] = macro.execute(&table, std::make_tuple(sel[0], sel[1], sel[2], sel[3]), source.str());
		if( rc == -1 ) {
			std::fprintf(stderr, "macro error: %s\n", err.c_str());
			return 1;
		}
		report("macro", start, table);
	}

	if( args.has("sort-col") ) {
		std::string type = args.get("sort-type", "str");
		int sortType = type == "num" ? 0 : type == "stri" ? 2 : 1;
		start = Clock::now();
		table.sortTable(std::stoi(args.get("sort-col")), !args.has("sort-desc"), sortType);
		report("sort", start, table);
		if( !save("save_sorted", table, out + ".sorted.csv") )
			return 1;
	}
	return 0;
}
