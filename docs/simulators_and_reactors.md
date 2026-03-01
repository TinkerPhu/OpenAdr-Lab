We need to clarify and reshape simulators and reactors.

## Device (a.k.a. Asset) Simulator:
### functions of time (evaluated at tick): 
a) time dependent value of power consumed (positive) or produced (negative) in watt:
this value is the simulation (that can later be replaced by a real power meter), caused by commands to actor and simulation of possible delay, grow or linear functions and variance. the function is always taking the previous current state as input value and creates a new current state. function to be on VEN UI graph (device power). also to be on graph: secondary quantities caused by device power like temperature and SOC.
b) externally decided setpoint input as rootcause for commands to actor. function to be on VEN UI graph (device SP). also secondary quantities like temperature SP and SOC SP.
### state:
quantities to be persisted on disk, mainly current power to calculate the next state (input for power function) and sometimes the power depends on external input that needs to be tracked and persisted as well, like SOC, temperature, daytime (to calculate delta t) which also needs persistance.
further state are dynamic limits (min and max) of those quantities. these are also variable with time (e.g. depending on iradiance, outside temperature, started non-interruptable washing machine)
### properties:
static properties: hard limits (min and max, some of the sources for dynamic limit calculations) for all state quantities, function of time parameters e.g. max change rate, delay, etc
power step sizes (stepless / steps / on-off)
duration step size (e.g. interruptable heating vs. non-interruptable washing machine program)
variable properties: default for the state quantities
predictable properties: forecasts like iradiance forecasts


## Reactor:
Reactor sets setpoint of device simulators (aka devices) by optimizing for three different goals: minimum cost, minimum carbonfootprint, multiple user comforts. 
those goals are prioritized and weighted.

receiving desired quantity requirements (mostly power) compares it with the current quantity of the devices and changes their setpoint based on a decision chain/tree.

the reactor has setpoint inputs on various levels: 
### top level
the top level input requires the reactor to decide and control all its devices and adjust their setpoints.
### device level
on device level, the input is specific targeted on the device, e.g. heating to 20 deg, charge power 11 kW etc.

### sorces of inputs:
- user override input (user wants to charge EV and override any VTN requests)
- top level VTN event (e.g. total power down, price rate, carbon rate), instananeous and forecasts
- device level VTN event (for a device quantity), instantaneous and forecasts
- indirect inputs like price incentives => reactor needs to create top level input
- default

### reactor state
reactor needs to calculate its current quantity state for each tick like a device. in fact, for the vtn, it is a device that can handle setpoint requests of various kinds and has state of current quantites. 
- the central state is VEN current power, the sum of all device current power states.
- other states are dynamic quantity limits like curently possible min and max. they depend on the static limits
- current price rate (based on default or VTN set price rate)
- current carbon rate (based on default or VTN set carbon rate)


### decision tree
decisions are based on priorities. only priorities with highest precedence (lowest value) are taken into account. if multiple equal priorities are at presence they are linearly averaged (weights, defaults or sub priorities might apply, especially for yes/no questions).

## priorities examples
- user override (top prio)
- vtn requests
- defaults

## order of decisions
1.) decide on the goal quantity optimization. this depends on user preference setting priorities for: price, carbon and comfort
2.) based on the input requests, the goal quantity is calculated
3.) the goal is then split into device goals. based on the dynamic limits and response time (dynamic change rate). devices goals are decided based on their order priorities, weights and steps sizes. multiple possible solutions/scenarios might occur.
4.) set setpoints to actors and forecast expectations by calculating the quantity function
5.) measure reality and compare with quantity functions
6.) adjust. there might be a new limit hit (weather changed, device down). adjust limit properties and start at 1.)

calculate for each device potential limits (within the possible limit ranges of the properties) for power, price, carbon (min and max)

---

it is probably not complete. it even might have contradictions in it. be my sparing partner and help me to complete it.
paraphrace and complete it with your oppinion and findings in a second document. Do not put too much weight on the current implementation logic, it is a patchwork and it is not working perfect. therefore we need to restructure or rewrite it.