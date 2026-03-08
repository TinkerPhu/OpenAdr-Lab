VEN needs to orchestrate and balance import, export, production, consumption (and part of those storage and recall) electrical energy. It needs to be able to calculate its current and to some extent it future price, co2 and comfort value. It needs to optimize for those in configured priorities. For that, it needs to arrange and order consumption and production. There are several hard and soft boundries like physical constant, calculable or probable ranges of consumers and producers, due times, controllable power sizes, controllable time step sizes and tiers. Not only should a VEN optimize for the above quantities, it should also provide flexibility options for VEN so that VTN can orchestrate VENs. A VEN needs to be prepared and adjust for sudden changes.



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

# Algorithm
Algorithm loop:
1. Set Time Plan Array (MinPlanTime, furthest user request, PlanTimeSteps)
2. Adjust all Rate and Export forecast inputs
3. Fit consumption into production, add up every asset power in every PlanTimeStep, calculate and optimize for Value (€, CO2,Comfort), document decissions per step
4. communicate to VTN
5. decice and execute current controll state

# Entities

## Instances
### Variables
PlannedRate: Array[RateSnapshot]
PastRate: Array[RateSnapshot]

PlannedEnergySum Array[EnergyPacket]
PastEnergySum Array[EnergyPacket]

Current State: ValueState

Assets: Array[Asset]

GetExportFlexibility(): Array[PowerSnapshot]
GetImportFlexibility(): Array[PowerSnapshot]


### Settings
MinPlanTime: 24h
PlanTimeSteps: 5min

etc.

### AssetProfile
PowerRange
AutoFill: bool
DefaultPriority
DefaultLeeway
DefaultValueCurve

etc.

## Structures (part of instances)

### RateSnapshot
TimeStamp

ExportPriceRate: Rate
ImportPriceRate: Rate
ImportCO2Rate: Rate


### Rate
Rate: float
Type: enum (per kWh, per month)

### PowerSnapshot
TimeStamp
PowerSnapshot

### EnergyPacket
Asset, ref

Priority

EarliestStart: TimeStamp
LatestStart: TimeStamp
LatestEnd: TimeStamp

PlannedPowerProfile: Array[EnergySnapshot]
PastPowerProfile: Array[EnergySnapshot]

ValueCurve: Array[ComfortRate]

TotalEnergy()
PlannedEnergy()
PastEnergy()
PlannedEnd()
Started()
ShortestDuration()
ShortestFreeEndTime()

#### Energy Packet
belongs to Asset with AssetLimits
Has Energy, StartTime and ComfortValue,
and derived from that: EndTime, Duration, Power, Price, CO2

### EnergySnapshot
TimeStamp
EnergyMeterSnapshot

### CalcCache
PriceSnapshot_cache
CO2Snapshot_cache
ComfortValueLimit€_cache
ComfortValueLimitCO2_cache

### ComfortRate (per kWh)
Percentage
ComfortValueLimit€
ComfortValueLimitCO2


### Asset
AssetState
AssetProfile
AssetHeuristics: Heuristics

### Heuristics

DefaultDaytimeHeuristic

etc

### AssetState
TimeStamp

Power

auto follow (bool)

Asset Priority

Comfort Value

SoC(t)

# Flexibility / Leeway

## User Flexibility

- Implicit Choice => Asset with repeating EnergyPacket: e.g. Cooking. Implicit, plan with heuristics

- As soon as possible => Price, CO2, EndTime, No VTN Flexibility

- As soon as possible for free => EndTime

- By EndTime for free => OK,

- By EndTime => Price, CO2

- For max this Price, CO2 => EndTime

## VTN Flexibility

- Export Flexibility based on SoC and PowerRange, conflicting with Planning (solve by Price, CO2 or ComfortValue)

- Export Flexibility: PowerExport cropping

- Import Flexibility based on Planning (solve by Price, CO2 or ComfortValue)


## Conflicts

conflicts should be solved according to priority and/or comfort value. Not sure yet how to resolve this.  
About VTN override events. Again, that depends on their value. Emergencies would cause penalties which have high value so that value would cause immediate change, maybe creating a new EnergySnapshot Instance in the Array. 
I am not clear yet, how many EnergySnapshots should be created, only corner points or sampling points like every 5 minutes, etc.

Again about conflict and priorities: I was hoping, everything might be solvable via comfort value: e.g. an emergency event is basically not compulsory, it is a matter of accepting consequences if not followed. consequences can mostly be expressed in fines, which ultimately means comfort money - if my comfort is worth more than the fine, I might just choose the fine. but of course the regular case is to follow the emergency event. So I think, the user can express priorities, where energy is free (generated by users PV or stored in its battery) and whenever free energy is not available, every EnergyPacket has a value to the user that she is willing to pay for (I call it comfort value). however, it isn't clear to me yet, how to express value for timing (e.g. "the EnergyPacket/'energy consuming task' should be done by tonight 19:00 for max €1, else do it until next Friday 18:00". Or "Just let me cook my meals as usual but warn me if it would cost more than €2". If there are clashes with requests, priorities either put them in sequence or in partially in parallel if the dosage adjustability is possible.

Does that make sense and is it possible to implement?

Do you have solution suggestions?




# TODO:
maybe rename EnergyPacket to EnergyTask - then again, NO. Task tends to imply one direction, like only energy usage and wouldnt fit for energy generation.