## Example Assets (Devices):

### Producer: PV
Power Range: 0 - 12kW max (produce)
Power Adjustability: fluctuating auto, croppable
Comfort Value: N/A [unclear]
Energy: fluctuating, daily forecast W(t) = ∫P(t)
Capacity: daily forecast Wtot = ∫Pforecast

Time Flexibility: None, forecast P(t)
Time Tiers: fluctuating

### Consumer/Producer: Battery
Power Range: -2kW - 2kW (consume/produce)
Power Adjustability: stepless, auto follower
Comfort Value: Value Curve
Energy: SoC (State of Charge) W(t) = η⋅∫Pcharge(t) - ∫Pdischarge
Capacity: 10kWh max, f(SoC)

Time Flexibility: stepless, Value Curve
Time Tiers: stepless, Multi Tiers

### Consumer: EV
Power Range: min 1.5kW, max 11kW (consume)
Power Adjustability: stepless, auto follower (10s delay)
Comfort Value: Value Curve
Energy: SoC W(t) = η⋅∫Pcharge(t), discharge fluctuating
Capacity: 50kWh max, f(SoC)

Time Flexibility: stepless, Value Curve
Time Tiers: stepless, Multi Tiers

### Consumer: Heater
Power Range: 6kW max (fluctuating consume)
Power Adjustability: 0kW, 3kW, 6kW, auto follower
Comfort Value: Value Curve
Energy: T = μ⋅η⋅∫Pcharge(t), discharge fluctuating
Capacity: 10kWh max, f(T)

Time Flexibility: stepless, Value Curve
Time Tiers: stepless, Multi Tiers

### Power Range: 3kW max (fluctuating consume)
Power Adjustability: ON/OFF
Comfort Value: Value Curve
Energy: T = μ⋅η⋅∫Pcharge(t), discharge fluctuating
Capacity: 10kWh max, f(T)

Time Flexibility: stepless, Value Curve
Time Tiers: min 0.5h, heuristic forecast P(t)

### Consumer: Washing Machine
Power Range: 2kW max (preset, fluctuating consume)
Power Adjustability: ON/OFF or Recommendation
Comfort Value: None [unclear, maybe f(t)
Energy: 1kWh
Capacity: 1kWh

Time Flexibility: stepless, [unclear, maybe Value Curve]
Time Tiers: 2h, heuristic forecast P(t)

### Consumer: Cooking Stove
Power Range: 3kW max (fluctuating consume)
Power Adjustability: None / Weak recommendation
Comfort Value: N/A [unclear]
Energy: discharge fluctuating, heuristic forecast W(t)
Capacity: 0.5kWh fluctuating

Time Flexibility: fix, heuristic forecast P(t), W(t)
Time Tiers: 0.5h, heuristic forecast P(t)

## Value Curve
While close to start of usage or storage, the value is high. When close to complete, the value to completion is low.
e.g.:
a) I am ready to invest €10 to get 60% because that is what I need, else it doesnt make sense at all. But if it is clear at the beginning that it won't suffice, give up at start. 
b) 80% would be fine but I don't really need it. If it is cheap, 70% for €5 is OK. More than 80% - I don't need it. But if it is for free, I take it anyway.

