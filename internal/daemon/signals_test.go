package daemon

import (
	"testing"

	"github.com/godbus/dbus/v5"
)

func TestRelevantSignalFiltersNetworkManagerNoise(t *testing.T) {
	active := &dbus.Signal{Name: propertiesInterface + ".PropertiesChanged", Body: []interface{}{nmName + ".Connection.Active", map[string]dbus.Variant{}, []string{}}}
	if !relevantSignal(active) {
		t.Fatal("active connection signal was filtered")
	}
	noise := &dbus.Signal{Name: propertiesInterface + ".PropertiesChanged", Body: []interface{}{nmName + ".Device", map[string]dbus.Variant{}, []string{}}}
	if relevantSignal(noise) {
		t.Fatal("unrelated NetworkManager signal was accepted")
	}
	added := &dbus.Signal{Name: objectManagerInterface + ".InterfacesAdded", Body: []interface{}{dbus.ObjectPath("/x"), map[string]map[string]dbus.Variant{nmName + ".Connection.Active": {}}}}
	if !relevantSignal(added) {
		t.Fatal("object addition signal was filtered")
	}
	noiseAdded := &dbus.Signal{Name: objectManagerInterface + ".InterfacesAdded", Body: []interface{}{dbus.ObjectPath("/x"), map[string]map[string]dbus.Variant{nmName + ".Device": {}}}}
	if relevantSignal(noiseAdded) {
		t.Fatal("unrelated object addition signal was accepted")
	}
	removed := &dbus.Signal{Name: objectManagerInterface + ".InterfacesRemoved", Body: []interface{}{dbus.ObjectPath("/x"), []string{nmName + ".Connection.Active"}}}
	if !relevantSignal(removed) {
		t.Fatal("active connection removal signal was filtered")
	}
}
