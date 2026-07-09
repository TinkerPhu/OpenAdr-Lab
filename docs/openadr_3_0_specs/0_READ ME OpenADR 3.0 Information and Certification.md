

# OpenADR 3.0 Introduction and Certification Program 

## **1 Introduction** 

The OpenADR 3.0 Standard (“OADR 3.0”) is not intended to replace the OpenADR 2.0a/b Profile Specifications. Rather, it provides an additional, simplified way to add OpenADR functionalities in current, as well as different and new scenarios. 

OADR 3.0 consists of the following components: 

1. **OpenADR 3.0 OpenAPI YAML (SwaggerDoc) Specification** : This is the **normative** reference for OADR 3.0 and supersedes any statements made in other documentation. If a user finds an inconsistency between the different documents, it should be reported to comments@openadr.org. 

A version of the YAML is provided in the public download package. 

2. **OpenADR 3.0 Definitions** : Defines and provides information models, Enumerations, Security and other aspects of OADR 3.0. The information model content duplicates what is in the YAML file, but is much easier to read in this form. 

The OpenADR 3.0 Definitions v3.0.0 is part of the public download package. 

3. **OpenADR 3.0 User Guide** : This document describes a number of common use scenarios of OpenADR 3.0, providing examples of program, event, reports, and endpoint usage. These examples are not prescriptive or normative but are provided as illustrations of how one might use the API. Further, as we are building the certification program, the implementations may play a role for certified solutions. These common interactions will drive interoperability. 

The OpenADR 3.0 Users Guide v3.0.0 is part of the public download package. 

### 4. **OpenADR 3.0 Reference Implementation** 

The OpenADR 3.0 Reference Implementation is available to OpenADR members only. Members of the Alliance can also request direct access to the github repository once it is established. 

## **2 Certification Program** 



### **General Aspects** 

The OpenADR 3.0 Standard, the OpenADR logo, and its certification marks are copyrighted and/or trademarked property of the OpenADR Alliance. Only products or systems that wen t through the OpenADR Alliance Certification Program can claim OpenADR compliance or certified status. 

OADR 3.0 will have several Certification Profiles for different application scenarios. This varies from the OADR 2.0 series where generally all functionalities had to be implemented. The goal is 

© OpenADR Alliance 2023 OpenADR Alliance, 111 Deerwood Road, Suite 200, San Ramon CA 94583, USA www.openadr.org 



to make implementations easier, in particular for downstream VENs and localized VTNs (in building). 

Example Certification Profiles are (more to be developed over time as needed): 

1. Price Receiving, Emergency Alert VEN 

2. Demand Flexibility VEN (not defined yet, just a possibility) 

3. EVSE Management VEN (not defined yet, just a possibility) 

4. Inverter Management VEN (not defined yet, just a possibility) 



### **VTN – Virtual Top Nodes** 

VTNs are the center of interoperability for OpenADR. Therefore, VTNs seeking certification status have to implement all features and Certification Profiles. However, the occurrence of new OpenADR 2.0a implementations has dropped significantly over the years. Therefore, we are dropping the requirement for VTNs to implement 2.0a. 

- Currently, VTNs can still become OpenADR certified by implementing OpenADR 2.0a and 2.0b. 

- After a grace period of 6 months from the publication of the initial set of Certification Profiles, commercial VTNs must implement OpenADR 2.0b and OpenADR 3.0 to obtain certification. 

- VTNs running in closed systems – e.g. VTNs purpose-build by a utility, aggregator, or government agency or in-building management systems, etc. – or that are not intended for commercialization and resale can apply to obtain certification for only OpenADR 2.0b or OpenADR 3.0 



### **VEN – Virtual End Node** 

VENs have the flexibility to implement any combination of Certification Profiles. However, at least one of the defined OpenADR 3.0 Certification Profiles must be implemented and tested to be considered for OpenADR 3.0 Certification. 

- Certification for OpenADR 2.0a and 2.0b VENs will remain unchanged. 

- Immediately after publication of the initial OADR 3.0 Certification Profiles, VENs can also become OpenADR 3.0 Certified by implementing at least one of the Profiles. 



### **OpenADR 3.0 Testing** 

OADR 3.0 testing will be conducted using scripts created by the alliance in collaboration with other vendors. It is subject to board discussions whether the test script will be made available to members as a membership benefit or if there will be a separate fee (alliance footing the bill vs. over time ROI). A final verification for certification must be conducted at one of the OpenADR Alliance appointed test houses. 



### **Certification Process** 

The certification process remains the same as for OpenADR 2.0 and EcoPort. 

© OpenADR Alliance 2023 OpenADR Alliance, 111 Deerwood Road, Suite 200, San Ramon CA 94583, USA www.openadr.org 

