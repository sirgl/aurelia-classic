#!/bin/sh
# One-shot, re-runnable, idempotent rename of the original game's name out of the
# project's own identifiers (classic/docs/rename-classic.md has the mapping and the rules).
#
#   Civ5 -> Classic, civ5 -> classic (types, modules, crates, paths, the module id)
#   civ5-rules|content|ui -> classic-rules|content|ui, civ-qa -> aurelia-qa
#   CIV_* environment variables -> AURELIA_* (CIV_RULES=civ5|lal -> AURELIA_RULES=classic|lands,
#                                             CIV_LAL_MODE -> AURELIA_LANDS_MODE)
#   core:civ/<x> -> core:nation/<x>, CIVSAVE/.civsave -> AURELIA/.aurelia,
#   CIVLOG/.civlog -> AURLOG/.aurlog, CIVMODS -> AURMODS
#   bnw (our ruleset) -> classic: classic/assets/rules/bnw -> classic/assets/rules/classic, bnw_* -> classic_*
#   docs/civ5 -> classic/docs/classic
#
# Usage:  sh classic/tools/rename-classic.sh            apply to the tracked files of the working tree,
#                                               then normalise Cargo.lock (cargo metadata --offline)
#         sh classic/tools/rename-classic.sh --repin    the second step, after the first one is committed: the one pin that
#                                               moves with the rename (the ruleset fingerprint, classic/crates/classic-rules
#                                               classic/mod.rs and tests/rules_fingerprint.rs). Idempotent as well.
#         sh classic/tools/rename-classic.sh --check    change nothing; exit 1 and list what the script
#                                               would still change (the naming guard; CI runs it)
#
# Only tracked files are touched (git ls-files); `ru/notes/`, `ru/` (the private archive of Russian originals), `assets/visual/` and `platform/third_party/`
# are left alone, and so is a line that contains the marker `rename-classic:keep`. Run it on a clean tree, review `git diff --stat`, commit. Running it again
# changes nothing. Saves written before the rename (.civsave) stop loading: accepted.
set -eu
ROOT="${AURELIA_ROOT:-$(git -C "$(dirname "$0")" rev-parse --show-toplevel)}"
cd "$ROOT"
export LC_ALL=C
if [ "${1:-}" = "--check" ]; then
  exec perl -x "$0" --check
fi
if [ "${1:-}" = "--repin" ]; then
  exec perl -x "$0" --repin
fi
perl -x "$0" --apply
# Cargo.lock lists packages by name: let cargo rewrite it in its own order (offline).
if command -v cargo >/dev/null 2>&1; then
  TOOLCHAIN="+1.98.1"
  cargo $TOOLCHAIN metadata --offline --format-version 1 >/dev/null 2>&1 || cargo metadata --offline --format-version 1 >/dev/null 2>&1 || echo "rename-classic: cargo metadata failed; run 'cargo metadata --offline' yourself to normalise Cargo.lock" >&2
fi
exit 0
#!perl
use strict;
use warnings;

my $mode = $ARGV[0] // '--apply';
my $check = $mode eq '--check';
my $repin = $mode eq '--repin';

my $SKIP = qr{^(?:ru/|[a-z]+/assets/visual/|platform/third_party/|target/|classic/tools/rename-classic\.sh$|classic/docs/rename-classic\.md$)};

# ---------------------------------------------------------------- paths
sub new_path {
    my $p = shift;
    $p =~ s/civ-qa/aurelia-qa/g;
    $p =~ s/civ_qa/aurelia_qa/g;
    $p =~ s/civsave/aurelia/g;
    $p =~ s/civlog/aurlog/g;
    $p =~ s/civdb/srcdb/g;
    $p =~ s/Civ5/Classic/g;
    $p =~ s/civ5/classic/g;
    $p =~ s/bnw/classic/g;
    return $p;
}

# ---------------------------------------------------------------- content
# Spans that must stay as they are: they name the original game's files, tables, wiki pages,
# our past branches and other people's local paths. Matched first, put back last.
my @PROTECT = (
    qr{https?://[^\s)>\]"'`]+},
    qr{Data/Civ5(?:/BNW)?},
    qr{CIV5\\?\[A-Za-z\]\\?\+\\\.xml\|Civ5\\?\[A-Z\]\\?\[A-Za-z\]\\?\+\\\.xml},
    qr{Civ5[A-Za-z]*\.xml},
    qr{CIV5[A-Za-z0-9_]*\.xml},
    qr{Civ5Pkg},
    qr{CIV5[A-Z][A-Za-z0-9_]*},
    qr{(?<![\w/.-])(?:integ/)?civ5/(?!generator\b)[a-z][a-z0-9-]*(?![\w*/-]|\.\w)},
    qr{(?:~|/home/\w+|/Users/[\w.]+|/Volumes/\w+)/[^\s`'")]+},
    qr{(?:units|inventory|generation-plan|fandom)-bnw},
    qr{Espionage \(Civ5\)},
    qr{(?<![\w-])bnw:(?=[a-z_]+/)},    # art ids of the art pack and its atlas indexes (bnw:unit/scout): assets/visual is not ours to rewrite
);

sub kind_of {
    my $p = shift;
    return 'rs'   if $p =~ /\.rs$/;
    return 'md'   if $p =~ /\.(?:md|txt)$/;
    return 'hash' if $p =~ /\.(?:py|sh|toml|yml|yaml)$/;
    return 'code';
}

# Standalone `Civ5` (not part of a longer identifier): in code it is the module's type and
# becomes `Classic`; in prose (a comment outside backticks, a markdown paragraph) it names the
# reference game and becomes `Civ V`, the citation form the public description allows.
sub standalone {
    my ($line, $kind, $in_fence) = @_;
    return $line unless $line =~ /\bCiv5\b/;
    if ($kind eq 'code' || $in_fence) {
        $line =~ s/\bCiv5\b/Classic/g;
        return $line;
    }
    my ($code, $prose) = ($line, '');
    if ($kind eq 'rs') {
        if ($line =~ m{^(.*?)(//.*)$}s) { ($code, $prose) = ($1, $2); }
        else { ($code, $prose) = ($line, ''); }
    } elsif ($kind eq 'hash') {
        if ($line =~ m{^(.*?(?:^|\s))(\#.*)$}s) { ($code, $prose) = ($1, $2); }
        else { ($code, $prose) = ($line, ''); }
    } elsif ($kind eq 'md') {
        ($code, $prose) = ('', $line);
    }
    $code =~ s/\bCiv5\b/Classic/g;
    my @seg = split /(`)/, $prose, -1;
    my ($out, $inside) = ('', 0);
    for my $s (@seg) {
        if ($s eq '`') { $inside = !$inside; $out .= $s; next; }
        if ($inside) { $s =~ s/\bCiv5\b/Classic/g; }
        else {
            $s =~ s/(?<=<)Civ5\b/Classic/g;
            $s =~ s/\bCiv5(?=::|>)/Classic/g;
            $s =~ s/\bCiv5\b/Civ V/g;
        }
        $out .= $s;
    }
    return $code . $out;
}

sub transform_line {
    my ($line, $kind, $in_fence) = @_;
    return $line unless $line =~ /civ|CIV|Civ|bnw|BNW|Bnw/;
    return $line if $line =~ /rename-classic:keep/;   # a line that has to name the old words (the inventory in SOURCES.md)
    my @prot;
    for my $re (@PROTECT) {
        $line =~ s/($re)/push @prot, $1; "\x01" . $#prot . "\x02"/ge;
    }
    # identifier forms first: Civ5Hud, SavedCiv5, Civ5_x
    $line =~ s/(?<=\w)Civ5/Classic/g;
    $line =~ s/Civ5(?=\w)/Classic/g;
    $line = standalone($line, $kind, $in_fence);

    $line =~ s/CIV_LAL_MODE/AURELIA_LANDS_MODE/g;
    $line =~ s/(CIV_RULES\b(?:["'\]]|\s)*[=:,]\s*["']?)lal\b/${1}lands/g;
    $line =~ s/(?<![A-Za-z0-9_])CIV_GAME_CIV\b/AURELIA_GAME_NATION/g;
    $line =~ s/(?<![A-Za-z0-9_])CIV_VISUAL_CIV\b/AURELIA_VISUAL_NATION/g;
    $line =~ s/(?<![A-Za-z0-9_])CIV_(SLUG|COEFFICIENT|CONSTANT)\b/NATION_$1/g;
    $line =~ s/(?<![A-Za-z0-9_])CIV_/AURELIA_/g;
    $line =~ s/CIVSAVE/AURELIA/g;
    $line =~ s/civsave/aurelia/g;
    $line =~ s/CIVLOG/AURLOG/g;
    $line =~ s/civlog/aurlog/g;
    $line =~ s/CIVMODS/AURMODS/g;
    $line =~ s/\b(core|test|mod):civ\//$1:nation\//g;
    $line =~ s/civ5-bnw-spec/classic-spec/g;
    $line =~ s/(temp_dir\(\)\.join\(format!\(")civ-/${1}aurelia-/g;   # scratch directories of the tests
    $line =~ s/\bciv-bench\b/aurelia-bench/g;                         # the bench line protocol tag
    $line =~ s/\bciv-content\b/classic-content/g;                      # a stale name of classic-content
    $line =~ s/\bciv-models\b/aurelia-models/g;                         # the schema tag of platform/assets/models/manifest.json
    $line =~ s/civ-qa/aurelia-qa/g;
    $line =~ s/civ_qa/aurelia_qa/g;
    $line =~ s/\bcivdb\b/srcdb/g;
    $line =~ s/CIV5_/CLASSIC_/g;
    $line =~ s/(?<![A-Za-z0-9_])CIV5(?![A-Za-z0-9_])/CLASSIC/g;
    $line =~ s/civ5/classic/g;
    $line =~ s/BNW_/CLASSIC_/g;
    $line =~ s/\bBnwGame\b/ClassicRun/g;   # a test's own struct: `ClassicGame` is the module's trait
    $line =~ s/Bnw/Classic/g;
    $line =~ s/bnw/classic/g;

    $line =~ s/\x01(\d+)\x02/$prot[$1]/g;
    return $line;
}


# The "Naming" section of classic/docs/classic/SOURCES.md after the rename: the intro, two table rows,
# the identifiers table and one bullet of the guard. The lines carry the `rename-classic:keep`
# marker because they have to name the old words.
my $KEEP = ' <!-- rename-classic:keep -->';
my $NAMING_INTRO = 'Decision (the project owner, via the coordinator, 2026-10-20): the game is nowhere called "Civilization". Publicly it is described as "Unciv / Civ 5 inspired game with a similar rule set"; the product name is **Aurelia** (window title in `lands/app/desktop/src/main.rs`, settings and cache folders). Citing Unciv as the source stays allowed. The project owner then decided that the project\'s own identifiers do not carry the original\'s name either: **done on 2026-10-20 by `classic/tools/rename-classic.sh`** (mapping, rules and the instructions for other branches: [../rename-classic.md](../rename-classic.md)). This section lists where the words Civilization, Civ, Civ V and civ5 occurred, what was fixed, and what the rename changed. Git history is not touched.';
my $NAMING_SAVES = '| Save and replay file extensions | were `.civsave`, `.civlog` (77 lines in 39 files) | **renamed** by the script: `.aurelia`, `.aurlog`; the magic bytes `CIVSAVE`, `CIVLOG`, `CIVMODS` became `AURELIA`, `AURLOG`, `AURMODS` (8 bytes as before). Saves written before do not load any more (accepted). |';
my $NAMING_REMAINING = '| Remaining "Civ V", "Civ 5" in crates and tools | comments, doc comments, test names, strings in tests: they name the reference game\'s rules, which the public description allows | keep, except where a rule is cited; the citations are the work of the re-sourcing plan. The script turned the prose word `Civ5` into `Civ V`; the identifier `Civ5` is now `Classic` |';
my $NAMING_TABLE = <<'EOT';
### Internal identifiers (renamed by `classic/tools/rename-classic.sh`)

| Identifier | Now | Note |
|---|---|---|
| Crates `civ5-rules`, `civ5-content`, `civ5-ui`, `civ-qa` | `classic-rules`, `classic-content`, `classic-ui`, `aurelia-qa` | directories, packages, path deps, `use`, `Cargo.lock`, `platform/tools/platform-lint.sh` (matches `classic-[a-z-]+`) |
| Rust types and modules `Civ5*`, module `civ5` | `Classic*`, module `classic` | `Rules::Classic`, `ClassicUi`, ... |
| Ruleset (module) id `civ5` | `classic` | enters the rules fingerprint and every pinned game hash: re-pinned once, see the re-pin commit |
| Content keys `core:civ/<x>` | `core:nation/<x>` | data, docs, tests, snapshots; enters the ruleset fingerprint |
| Environment variables `CIV_*` | `AURELIA_*` (`CIV_RULES=civ5|lal` -> `AURELIA_RULES=classic|lands`, `CIV_LAL_MODE` -> `AURELIA_LANDS_MODE`) | no fallback to the old names |
| Save and replay files | `.aurelia` / `AURELIA`, `.aurlog` / `AURLOG`, slot tag `AURMODS` | old local saves stop loading |
| Rule-set files named after the original | `classic/docs/classic/`, `classic/assets/rules/classic/`, `bnw_*` -> `classic_*`, `civ5*` file names | |
| Git remote and repository name `civ-visual-sandbox`, branch names, local folders `~/civ` | unchanged | the public URL of the repository: The project owner's decision; the split plan names the new repository `aurelia-lands`; `lal-*` crates keep their names |

Kept on purpose and checked by `sh classic/tools/rename-classic.sh --check` (the list `PROTECT` at the top of the script): the original game's own file and table names (`CIV5*` tables, `Civ5Pkg`, `Data/Civ5/BNW` of the wiki dump), URLs, the names of our past branches (`civ5/unit-quality`), other people's local paths, `fandom-bnw-dump`, the atlas file names `units-bnw-*`.
EOT
my $NAMING_GUARD = '- `sh classic/tools/rename-classic.sh --check` is the naming guard (classic/docs/rename-classic.md): it fails when the old names of the project\'s own identifiers (see the table above) come back. Its allowlist is the `PROTECT` list in the script and the marker `rename-classic:keep`.';

sub patch_sources {
    my $text = shift;
    my @L = split /\n/, $text, -1;
    my @out;
    my $skip = 0;
    for my $l (@L) {
        if ($skip) {
            next unless $l =~ /^## Guard/;
            $skip = 0;
        }
        if ($l =~ /^Decision \(the project owner, via the coordinator/) { push @out, $NAMING_INTRO . $KEEP; next; }
        if ($l =~ /^\| Save and replay file extensions \|/) { push @out, $NAMING_SAVES . $KEEP; next; }
        if ($l =~ /^\| Remaining "Civ V"/) { push @out, $NAMING_REMAINING . $KEEP; next; }
        if ($l =~ /^### Internal identifiers/) {
            for my $t (split /\n/, $NAMING_TABLE) { push @out, ($t =~ /^\|[- |]+$/ || $t eq '' || $t =~ /^###/ || $t =~ /^\| Identifier/ ? $t : $t . $KEEP); }
            push @out, '';
            $skip = 1;
            next;
        }
        if ($l =~ /^- It compares the number of matching lines/ && !grep { /rename-classic\.sh --check. is the naming guard/ } @out) {
            push @out, $NAMING_GUARD;
        }
        push @out, $l;
    }
    return join("\n", @out);
}

# Edits that are not a plain word swap. Each matches only the old text, so it is idempotent.
sub patch {
    my ($path, $text) = @_;
    if ($path eq 'lands/app/desktop/src/module.rs') {
        $text =~ s/Ok\("lal"\) => Rules::Lal/Ok("lands") => Rules::Lal/g;
        $text =~ s/ModuleId\s*\{\s*name:\s*"civ"/ModuleId { name: "classic"/g;
    }
    if ($path =~ m{^platform/crates/client/src/shell/saves/(?:slot|autosave)\.rs$}) {
        $text =~ s/ModuleId \{ name: "civ"/ModuleId { name: "classic"/g;
        $text =~ s/running: "civ"/running: "classic"/g;
        $text =~ s/\\0\\0\\0civ"/\\0\\0\\0classic"/g;
        $text =~ s/\bCIV\b/CLASSIC/g;
    }
    if ($path eq 'platform/crates/client/src/shell/saves/slot.rs') {
        $text =~ s/this game runs the civ rules/this game runs the classic rules/g;
    }
    if ($path =~ m{^classic/crates/classic-ui/src/ui/screen_pedia\.rs$}) {
        $text =~ s/From the Civ V catalog; this build/From the ruleset catalog; this build/g;
    }

    if ($path eq '.github/workflows/ci.yml' && $text !~ /Naming guard/) {
        $text =~ s/(      - name: Platform lint\n        run: sh tools\/platform-lint\.sh\n)/$1      - name: Naming guard (no old names; docs\/rename-classic.md)\n        run: sh tools\/rename-classic.sh --check\n/;
    }
    if ($path eq 'classic/tools/sources-lint.sh') {
        $text =~ s/(:!tools\/unciv-coverage\.py)'/$1 :!tools\/rename-classic.sh :!docs\/rename-classic.md'/ unless $text =~ /rename-classic\.sh :!docs/;
        # the naming guard: the old names (civ5, Civ5, CIV_*, core:civ/, civsave, bnw ...) must not come back
        my $guard = "# Naming guard (classic/docs/rename-classic.md): the old names of the own identifiers (civ5, Civ5, CIV_*, core:civ/) must not come back. rename-classic:keep\n"
                  . "sh classic/tools/rename-classic.sh --check || {\n"
                  . "  echo \"sources-lint: FAILED (naming guard above)\" >&2\n"
                  . "  exit 1\n"
                  . "}\n\n";
        $text =~ s/^(if \[ ! -f "\$BASELINE" \]; then\n)/$guard$1/m unless $text =~ /rename-classic\.sh --check/;
    }
    if ($path eq 'classic/docs/classic/SOURCES.md') {
        $text = patch_sources($text);
    }
    return $text;
}

sub transform {
    my ($path, $text) = @_;
    my $kind = kind_of($path);
    my @lines = split /(?<=\n)/, $text;
    my $fence = 0;
    for my $l (@lines) {
        if ($kind eq 'md' && $l =~ /^\s*```/) { $fence = !$fence; }
        $l = transform_line($l, $kind, $fence);
    }
    return patch($path, join('', @lines));
}

# ---------------------------------------------------------------- re-pin
# The ruleset fingerprint hashes the module id and the content keys, so it is the one pin that moves
# with the rename (game hashes, world hashes and snapshots do not read names). The old value joins the
# list of earlier editions the snapshot loader accepts. A branch whose own content changes moved the
# fingerprint to another value: this step does not match it, the test prints the new value, pin it by hand.
my ($OLD_FP, $NEW_FP) = ('11011276917743099964', '10671736804218944378');

sub repin_text {
    my ($path, $text) = @_;
    if ($path eq 'classic/crates/classic-rules/src/classic/mod.rs') {
        $text =~ s/(pub const CLASSIC_FINGERPRINT: u64 = )$OLD_FP;/$1$NEW_FP;/;
        $text =~ s/(        \(16018243681386759787, CLASSIC_FINGERPRINT\),\n)(    \];)/$1        ($OLD_FP, CLASSIC_FINGERPRINT),\n$2/
            unless $text =~ /\($OLD_FP, CLASSIC_FINGERPRINT\)/;
        $text =~ s{^    /// load under\n}{    /// and the edition before the rename (its module id and content keys carried the original\n    /// game's name) load under\n}m;
    }
    if ($path eq 'classic/crates/classic-rules/tests/rules_fingerprint.rs') {
        $text =~ s/(assert_eq!\(CURRENT, )$OLD_FP\);/$1$NEW_FP);/;
        $text =~ s/const EARLIER: \[u64; 10\] = \[/const EARLIER: [u64; 11] = [/;
        $text =~ s/(    16018243681386759787,\n)(\];)/$1    $OLD_FP,\n$2/ unless $text =~ /^    $OLD_FP,$/m;
        $text =~ s{^/// keeps its deposits\)\.\nconst EARLIER}{/// keeps its deposits) and the one before the rename ($OLD_FP: the module id\n/// and the content keys, which the hash covers, carried the original game's name then).\nconst EARLIER}m;
    }
    return $text;
}

if ($repin) {
    my $n = 0;
    for my $p ('classic/crates/classic-rules/src/classic/mod.rs', 'classic/crates/classic-rules/tests/rules_fingerprint.rs') {
        open(my $fh, '<:raw', $p) or die "open $p: $!\n";
        local $/;
        my $text = <$fh>;
        close $fh;
        my $new = repin_text($p, $text);
        next if $new eq $text;
        open(my $out, '>:raw', $p) or die "write $p: $!\n";
        print $out $new;
        close $out;
        $n++;
    }
    print "rename-classic --repin: $n files re-pinned\n";
    exit 0;
}

# ---------------------------------------------------------------- driver
sub tracked {
    my $out = `git ls-files -z`;
    die "git ls-files failed\n" if $? != 0;
    return grep { length && !/$SKIP/ } split /\0/, $out;
}

my @dirty;
my ($n_moved, $n_changed, $n_bin) = (0, 0, 0);

my @moved;
for my $p (tracked()) {
    my $np = new_path($p);
    next if $np eq $p;
    if ($check) { push @dirty, "path: $p -> $np"; next; }
    die "rename-classic: $np already exists; resolve by hand\n" if -e $np;
    (my $dir = $np) =~ s{/[^/]*$}{};
    system('mkdir', '-p', $dir) == 0 or die "mkdir $dir failed\n";
    rename($p, $np) or die "rename $p -> $np: $!\n";
    push @moved, $p, $np;
    $n_moved++;
}
if (@moved) {
    # one git call for all moves (a git mv per file is slow on a big tree)
    open(my $git, '|-', 'git', 'add', '-A', '--pathspec-from-file=-', '--pathspec-file-nul') or die "git add: $!\n";
    print $git join("\0", @moved), "\0";
    close($git) or die "git add failed\n";
}

for my $p (tracked()) {
    next unless -f $p && !-l $p;
    open(my $fh, '<:raw', $p) or die "open $p: $!\n";
    local $/;
    my $text = <$fh>;
    close $fh;
    my $new;
    if (index($text, "\0") >= 0) {
        # binary: only the magic of a save or a journal fixture
        $new = $text;
        if ($p =~ /\.aurelia$/) { $new =~ s/^CIVSAVE\0/AURELIA\0/; }
        if ($p =~ /\.journal$/) { $new =~ s/^CIVLOG\0/AURLOG\0/; }
        $n_bin++ if $new ne $text && !$check;
    } else {
        $new = transform($p, $text);
    }
    next if $new eq $text;
    if ($check) {
        my @a = split /(?<=\n)/, $text;
        my @b = split /(?<=\n)/, $new;
        my $shown = 0;
        for my $i (0 .. $#a) {
            next if defined $b[$i] && $a[$i] eq $b[$i];
            my $l = $a[$i]; chomp $l; $l = substr($l, 0, 140);
            push @dirty, sprintf("%s:%d: %s", $p, $i + 1, $l);
            last if ++$shown >= 5;
        }
        push @dirty, "$p: (binary magic)" unless @a;
        next;
    }
    open(my $out, '>:raw', $p) or die "write $p: $!\n";
    print $out $new;
    close $out;
    $n_changed++;
}

if ($check) {
    if (@dirty) {
        print STDERR "naming guard: the tracked tree still carries the old names (run: sh classic/tools/rename-classic.sh):\n";
        print STDERR "  $_\n" for @dirty[0 .. ($#dirty < 199 ? $#dirty : 199)];
        print STDERR "  ... and more\n" if @dirty > 200;
        exit 1;
    }
    print "naming guard: ok (the rename script would change nothing)\n";
    exit 0;
}
print "rename-classic: $n_moved paths moved, $n_changed files rewritten ($n_bin binary fixtures re-magicked)\n";
