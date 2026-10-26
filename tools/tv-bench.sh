#!/bin/sh
# Frame pacing of the installed Lounge on the TV, for comparing builds.
#
#   ssh lg-tv 'sh -s' < tools/tv-bench.sh            # every scenario
#   ssh lg-tv 'sh -s grid "--skip occlusion"' < tools/tv-bench.sh
#
# Runs the installed binary under the developer jailer with a scripted path,
# three times per scenario, 1000 presented frames each, and prints the median
# of each run's mean, p95 and slow-frame count (single runs vary too much on
# this TV to compare builds). Run it only while nobody is watching: Lounge is
# restarted, and an app left in the background may be reloaded by webOS.
# Ends by relaunching Lounge cleanly.
APP=/media/developer/apps/usr/palm/applications/io.github.yashwanthkamireddi.lounge
ONLY=$1
EXTRA=$2

# Never leave a benchmark copy of Lounge running beside the real one: on exit
# or interruption it is stopped and the real Lounge relaunched.
finish() {
	for pid in $(pgrep -f '[j]ailer -t native_devmode -i io.github.yashwanthkamireddi.lounge'); do kill "$pid"; done
	for pid in $(pgrep -f '[-]-profile-frames'); do kill "$pid"; done
	rm -f /tmp/lounge-bench.json
	luna-send -n 1 luna://com.webos.applicationManager/closeByAppId '{"id":"io.github.yashwanthkamireddi.lounge"}' >/dev/null
	sleep 2
	luna-send -n 1 luna://com.webos.applicationManager/launch '{"id":"io.github.yashwanthkamireddi.lounge"}' >/dev/null
}
trap finish EXIT
trap 'exit 130' INT TERM HUP
# A copy left by an interrupted run goes first.
for pid in $(pgrep -f '[-]-profile-frames'); do kill "$pid"; done

scenario() {
	name=$1
	script=$2
	[ -n "$ONLY" ] && [ "$ONLY" != "$name" ] && return
	results=""
	for run in 1 2 3; do
		luna-send -n 1 luna://com.webos.applicationManager/closeByAppId '{"id":"io.github.yashwanthkamireddi.lounge"}' >/dev/null
		sleep 1.5
		rm -f /tmp/lounge-bench.json
		(cd /tmp && XDG_RUNTIME_DIR=/tmp/xdg timeout 120 /usr/bin/jailer -t native_devmode -i io.github.yashwanthkamireddi.lounge -p "$APP" \
			"$APP/lounge" '{"@system_native_app":true,"nid":"io.github.yashwanthkamireddi.lounge"}' \
			--script "$script" --profile-frames 1000 $EXTRA --diagnostics /tmp/lounge-bench.json >/dev/null 2>&1)
		line=$(python3 -c "
import json
d = json.load(open('/tmp/lounge-bench.json'))
p = d['profile']
print(p['pacing']['mean_ms'], p['pacing']['p95_ms'], len(p['slow_frames']), d['display_mode']['refresh_hz'])
" 2>/dev/null)
		results="$results$line;"
	done
	python3 -c "
import sys
runs = [r.split() for r in sys.argv[2].split(';') if r.strip()]
if not runs:
    print('%-8s no result' % sys.argv[1]); sys.exit()
def median(values):
    values = sorted(values)
    return values[len(values) // 2]
mean = median(float(r[0]) for r in runs)
p95 = median(float(r[1]) for r in runs)
slow = median(int(r[2]) for r in runs)
print('%-8s %s Hz  mean %.1f ms  p95 %.1f ms  slow %d / 1000  (runs: %s)' % (
    sys.argv[1], runs[0][3], mean, p95, slow, ' '.join(r[2] for r in runs)))
" "$name" "$results"
}

scenario dock "wait,wait,right,wait,right,wait,right,wait,left,wait,left,wait,left,wait,wait"
scenario grid "wait,wait,down,wait,down,wait,down,wait,down,wait,up,wait,up,wait,up,wait,up,wait,wait"
scenario move "wait,wait,down,wait,hold,wait,right,wait,right,wait,down,wait,left,wait,up,wait,enter,wait,wait"
scenario switch "wait,wait,back,wait,right,wait,right,wait,left,wait,back,wait,wait"

